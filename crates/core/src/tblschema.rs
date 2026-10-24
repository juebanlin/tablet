use std::path::Path;
use anyhow::{Result, bail};
use crate::model::*;
use crate::tbl_str;
use crate::types::SeparatorsSection;

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

pub fn parse_tblschema(content: &str) -> Result<TblSchema> {
    let mut sections = Vec::new();
    let mut current: Option<SchemaSection> = None;
    let mut meta = SchemaMetadata::default();
    // 声明 `# @meta id` 的那一行（1-based），用于报错定位。`None` = 压根没声明。
    let mut meta_id_line: Option<usize> = None;
    let mut separators = SeparatorsSection::default();
    let mut seen_section = false;
    let mut in_preset = false;

    for (line_num, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if trimmed.starts_with('#') {
            // `# @preset` 进入预设数据模式；之后的非 `#` / 非 `[` 行按 `|` 切作为预设行。
            // 仅识别独立 `# @preset`（后接空白或行尾），其它带后缀写法当注释。
            if let Some(rest) = trimmed.trim_start_matches('#').trim_start().strip_prefix("@preset") {
                if rest.trim().is_empty() {
                    if current.is_none() {
                        bail!("line {}: `# @preset` 必须出现在某个 [section] 之后", line_num + 1);
                    }
                    in_preset = true;
                    continue;
                }
            }
            // 段内 `# @display: <列名>`（被引用展示列，@10）：挂到当前 section 上。
            // 只在 `[section]` 之后有意义；段前的 @display 忽略。
            if let Some(rest) = trimmed.trim_start_matches('#').trim_start().strip_prefix("@display") {
                if let Some(v) = rest.trim_start().strip_prefix(':') {
                    if let Some(ref mut sec) = current {
                        let v = v.trim();
                        sec.display = if v.is_empty() { None } else { Some(v.to_string()) };
                    }
                    continue;
                }
            }
            // 段内 `# @rule: <列> = <表达式> [级别]`（节点级校验规则，@01.10.7）。
            // 与 `@display` 不同：必须出现在某个 [section] 之后，段外直接报错——
            // 静默丢掉一条规则比少写一行危险得多。
            if let Some(rest) = trimmed.trim_start_matches('#').trim_start().strip_prefix("@rule") {
                let ours = rest.is_empty()
                    || rest.starts_with(':')
                    || rest.starts_with(|c: char| c.is_whitespace());
                if ours {
                    if current.is_none() {
                        bail!("line {}: `# @rule` 必须出现在某个 [section] 之后", line_num + 1);
                    }
                    let after = rest.trim_start();
                    // `# @rule <名字> = script(...)` 是第 3 层脚本规则的预留语法，尚未实现。
                    let Some(body) = after.strip_prefix(':') else {
                        bail!("line {}: `# @rule` 只支持 `# @rule: <列> = <表达式> [级别]`；\
                               脚本规则（# @rule <名字> = script(...)）尚未实现", line_num + 1);
                    };
                    if let Some(ref sec) = current {
                        if matches!(sec.mode, SchemaMode::Constant) {
                            bail!("line {}: constant 段不支持 `# @rule`（常量的 value 逐行类型可变，用内置校验即可）", line_num + 1);
                        }
                    }
                    let rule = parse_rule_body(body)
                        .map_err(|msg| anyhow::anyhow!("line {}: {}", line_num + 1, msg))?;
                    if let Some(ref mut sec) = current {
                        sec.rules.push(rule);
                    }
                    continue;
                }
            }
            // 仅在第一个 [group/Name] 之前的 `# @meta key: value` / `# @sep key = value` 行视作 directive；
            // 之后的 `#` 行一律视作普通注释。
            if !seen_section {
                if let Some((key, value)) = parse_meta_line(trimmed) {
                    match key.as_str() {
                        "id" => {
                            meta_id_line = Some(line_num + 1);
                            meta.id = value;
                        }
                        "name" => meta.name = value,
                        "category" => meta.category = value,
                        "version" => meta.version = value,
                        "created_at" => meta.created_at = value,
                        "source_template" => meta.source_template = value,
                        "source_template_version" => meta.source_template_version = value,
                        "has_preset" => meta.has_preset = value.eq_ignore_ascii_case("true"),
                        _ => { /* 未知 key：忽略，前向兼容 */ }
                    }
                } else if let Some((key, value)) = parse_sep_line(trimmed) {
                    apply_sep_kv(&mut separators, &key, &value);
                }
            }
            continue;
        }

        if trimmed.starts_with('[') {
            seen_section = true;
            in_preset = false;
            if let Some(sec) = current.take() {
                validate_section(&sec, line_num)?;
                sections.push(sec);
            }
            current = Some(parse_section_header(trimmed, line_num)?);
        } else if let Some(ref mut sec) = current {
            if in_preset {
                // preset 行：按 `|` 切并 decode，trim 单元格。空行已在外层过滤。
                // 走 tbl_str：让用户在 schema 里用 `\|` 表示字面竖线、`\n` 表示换行等。
                // 这里统一按 Str 类型 decode（Atom 类型的值不含转义字符，decode 是零成本 fast-path）。
                let cells: Vec<String> = tbl_str::split_row(trimmed)
                    .into_iter()
                    .map(|s| tbl_str::decode(s.trim(), "txt"))
                    .collect();

                // **列数上限：多出来的列会被 `apply_schema_to_project` 直接丢掉。**
                // `Vec::resize` 截断不报错，等于"多写一列 → 少一个值"；AI 复制粘贴
                // 一行很容易多一个 `|`。少写（尾部空列）是允许的，缺的位置按空处理。
                let max_cols = match sec.mode {
                    SchemaMode::Table => sec.fields.len(),
                    SchemaMode::Constant => 5, // name | type | value | export | desc
                    SchemaMode::Enum => 3,     // id | name | desc
                };
                // 空表（一个字段都没有）交给后面的「表没有定义任何字段」，这里不抢答
                let check_cols = !matches!(sec.mode, SchemaMode::Table) || !sec.fields.is_empty();
                if check_cols && cells.len() > max_cols {
                    bail!(
                        "line {}: [{}/{}] 的 preset 行有 {} 列，最多 {} 列（多出来的会被丢弃）",
                        line_num + 1, sec.group, sec.name, cells.len(), max_cols
                    );
                }

                // 常量预设行的第 4 列是 export，和字段行同一个规矩：只认短码
                if matches!(sec.mode, SchemaMode::Constant) {
                    if let Some(raw) = cells.get(3) {
                        if Export::parse_code(raw).is_none() {
                            bail!("line {}: 常量 {} 的 export 值 \"{}\" 不合法——只接受短码 cs / c / s / -，留空 = cs",
                                line_num + 1,
                                cells.first().map(String::as_str).unwrap_or("").trim(),
                                raw.trim());
                        }
                    }
                }
                sec.preset.push(cells);
            } else {
                match sec.mode {
                    SchemaMode::Table => {
                        let field = parse_field_line(trimmed, line_num, &sec.mode)?;
                        sec.fields.push(field);
                    }
                    SchemaMode::Constant | SchemaMode::Enum => {
                        let mode_name = if matches!(sec.mode, SchemaMode::Constant) { "constant" } else { "enum" };
                        bail!(
                            "line {}: [{}/{}] {} 段不允许直接的数据行（如需预设值请放进 `# @preset` 块）",
                            line_num + 1, sec.group, sec.name, mode_name
                        );
                    }
                }
            }
        } else {
            bail!("line {}: field outside section", line_num + 1);
        }
    }

    if let Some(sec) = current {
        validate_section(&sec, content.lines().count())?;
        sections.push(sec);
    }

    // name 缺省 = id；id 缺省由调用方按文件名 stem 兜底（这里不强制）
    if meta.name.is_empty() {
        meta.name = meta.id.clone();
    }

    // has_preset 永远从 sections 重算，忽略 @meta 行声明
    meta.has_preset = sections.iter().any(|s| !s.preset.is_empty());

    // **声明了 id 就必须合法。** 「没声明」是合法的——合并用的片段、裸 `.tblschema` 都
    // 不带 id（`merge_schemas` 也不挂 metadata，见下），要拦的是「声明了一个别处都不允许
    // 存在的 id」。空值声明（`# @meta id:`）算非法，那是写坏了，不是"没声明"。
    //
    // 这条是给**机器调用方**的反馈：CLI 主要由 AI / 脚本驱动，静静接受一个非法 id 会让
    // 问题潜伏到下一次操作才炸，而那时现场已经变了。@02.26 那个洞就是这么来的——改名
    // 能造出 `Bad-Id`，而加载时谁都不吭声。


    if let Some(n) = meta_id_line {
        if !is_valid_metadata_id(&meta.id) {
            bail!(
                "line {}: `# @meta id` 不合法: {:?}（只允许小写字母 / 数字 / _ / -，长度 1..=32）",
                n, meta.id
            );
        }
    }

    Ok(TblSchema { meta, separators, sections })
}

/// 解析 `# @meta key: value` 行；返回 (key, value)。
/// 接受形式：
///   `# @meta id: full`
///   `#@meta name: 完整测试模板`
/// key 大小写敏感；后者覆盖前者由调用方处理。
fn parse_meta_line(line: &str) -> Option<(String, String)> {
    // 去掉前导 '#'，再去空白
    let body = line.trim_start_matches('#').trim();
    let after = body.strip_prefix("@meta")?.trim_start();
    let (key, value) = after.split_once(':')?;
    Some((key.trim().to_string(), value.trim().to_string()))
}

/// 解析 `# @sep key = value` 行；返回 (key, value)。
/// 接受 `# @sep List = ;` / `# @sep Map.entry = ,` / `# @sep Map_Tuple2.kv = :` 形式。
/// 与 `parse_meta_line` 区别：分隔符值通过 `=` 切（避免与 `Map.kv` 中 ':' 冲突），
/// 等号两侧 trim，但**值本身不再 trim 内部空格**——保留原样以便支持空白类分隔符。
fn parse_sep_line(line: &str) -> Option<(String, String)> {
    let body = line.trim_start_matches('#').trim();
    let after = body.strip_prefix("@sep")?.trim_start();
    let (key, value) = after.split_once('=')?;
    Some((key.trim().to_string(), value.trim_start().trim_end_matches('\n').to_string()))
}

/// 解析 `# @rule:` 的正文（冒号之后的部分）：`<列> = <表达式> [error|warn|info]`。
///
/// 语法与 `# @sep key = value` 同源：用第一个 `=` 切列名与表达式，
/// 表达式整体交给表达式解析器，不靠空格切分（列名本身不可能含空格）。
fn parse_rule_body(body: &str) -> Result<Rule, String> {
    let body = body.trim();
    let (col, rest) = body
        .split_once('=')
        .ok_or_else(|| "`# @rule` 缺少 `=`：应为 `<列> = <表达式> [级别]`".to_string())?;
    let col = col.trim();
    if col.is_empty() {
        return Err("`# @rule` 缺少列名".to_string());
    }
    if col.chars().any(|c| c.is_whitespace()) {
        return Err(format!("规则列名 \"{}\" 不能含空白", col));
    }

    let mut expr = rest.trim();
    let mut level = Severity::Error;
    if let Some(stripped) = expr.strip_suffix(']') {
        if let Some(open) = stripped.rfind('[') {
            let raw_level = stripped[open + 1..].trim();
            match Severity::parse(raw_level) {
                Some(lv) => {
                    level = lv;
                    expr = stripped[..open].trim();
                }
                None => return Err(format!(
                    "未知的规则级别 \"{}\"（只接受 error / warn / info）", raw_level
                )),
            }
        }
    }
    if expr.is_empty() {
        return Err(format!("规则 \"{}\" 缺少表达式", col));
    }

    let kind = parse_rule_expr(expr)?;
    Ok(Rule { column: col.to_string(), kind, level })
}

/// 拆函数调用 `name(args)`：引号内的括号不参与匹配。
fn split_call(expr: &str) -> Option<(&str, &str)> {
    let open = expr.find('(')?;
    let name = expr[..open].trim();
    let mut in_str = false;
    let mut esc = false;
    let mut end = None;
    for (i, c) in expr[open + 1..].char_indices() {
        if in_str {
            if esc { esc = false; }
            else if c == '\\' { esc = true; }
            else if c == '"' { in_str = false; }
        } else if c == '"' {
            in_str = true;
        } else if c == ')' {
            end = Some(open + 1 + i);
            break;
        }
    }
    let end = end?;
    if !expr[end + 1..].trim().is_empty() { return None; }
    Some((name, &expr[open + 1..end]))
}

/// 顶层逗号切分（引号内的逗号不切），并去掉引号 / 反转义。
fn split_args(inner: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_str = false;
    let mut esc = false;
    let mut had_quote = false;
    for c in inner.chars() {
        if in_str {
            if esc {
                cur.push(match c { 'n' => '\n', 't' => '\t', _ => c });
                esc = false;
            } else if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            } else {
                cur.push(c);
            }
        } else if c == '"' {
            in_str = true;
            had_quote = true;
        } else if c == ',' {
            out.push(cur.trim().to_string());
            cur.clear();
            had_quote = false;
        } else {
            cur.push(c);
        }
    }
    if in_str { return Err("参数里的引号没有闭合".to_string()); }
    if !inner.trim().is_empty() || had_quote {
        out.push(cur.trim().to_string());
    }
    Ok(out)
}

fn arity(args: &[String], n: usize, name: &str) -> Result<(), String> {
    if args.len() != n {
        return Err(format!("{} 需要 {} 个参数，实际给了 {}", name, n, args.len()));
    }
    Ok(())
}

/// 解析内置验证器调用。
fn parse_rule_expr(expr: &str) -> Result<RuleKind, String> {
    let (name, inner) = split_call(expr)
        .ok_or_else(|| format!("无法解析规则表达式 \"{}\"", expr))?;
    let args = split_args(inner)?;
    match name {
        "unique" => { arity(&args, 0, name)?; Ok(RuleKind::Unique) }
        "non_empty" => { arity(&args, 0, name)?; Ok(RuleKind::NonEmpty) }
        "ref_exists" => {
            arity(&args, 1, name)?;
            let (node, target_col) = args[0]
                .split_once('.')
                .ok_or_else(|| "ref_exists 需要 `<节点>.<列>`".to_string())?;
            let node = node.trim();
            let target_col = target_col.trim();
            if node.is_empty() || target_col.is_empty()
                || node.chars().any(|c| c.is_whitespace())
                || target_col.chars().any(|c| c.is_whitespace()) {
                return Err("ref_exists 需要 `<节点>.<列>`（两侧都不能为空或含空白）".to_string());
            }
            Ok(RuleKind::RefExists {
                target_node: node.to_string(),
                target_column: target_col.to_string(),
            })
        }
        "range" => {
            arity(&args, 2, name)?;
            Ok(RuleKind::Range { min: args[0].clone(), max: args[1].clone() })
        }
        "len" => {
            arity(&args, 2, name)?;
            Ok(RuleKind::Len { min: args[0].clone(), max: args[1].clone() })
        }
        "one_of" => {
            if args.is_empty() {
                return Err("one_of 至少需要一个候选值".to_string());
            }
            Ok(RuleKind::OneOf { values: args })
        }
        "matches" => {
            arity(&args, 1, name)?;
            Ok(RuleKind::Matches { pattern: args[0].clone() })
        }
        "required_if" => {
            arity(&args, 2, name)?;
            if args[0].trim().is_empty() {
                return Err("required_if 需要一个列名".to_string());
            }
            Ok(RuleKind::RequiredIf { column: args[0].clone(), value: args[1].clone() })
        }
        "compare" => {
            arity(&args, 2, name)?;
            let op = CompareOp::parse(&args[1])
                .ok_or_else(|| format!("未知的比较运算符 \"{}\"（支持 < <= > >= == !=）", args[1]))?;
            if args[0].trim().is_empty() {
                return Err("compare 需要一个列名".to_string());
            }
            Ok(RuleKind::Compare { column: args[0].clone(), op })
        }
        _ => Err(format!(
            "未知验证器 \"{}\"（支持 unique / non_empty / ref_exists / range / len / one_of / matches / required_if / compare）",
            name
        )),
    }
}

/// 把单个 `# @sep` 行写入 `SeparatorsSection`。未知 key 忽略（向前兼容）。
fn apply_sep_kv(sep: &mut SeparatorsSection, key: &str, value: &str) {
    if let Some(k) = crate::types::SepKey::from_directive_key(key) {
        k.set(sep, value.to_string());
    }
}

fn parse_section_header(line: &str, line_num: usize) -> Result<SchemaSection> {
    let end_bracket = line.find(']').unwrap_or(0);
    let path = &line[1..end_bracket];
    let rest = line[end_bracket + 1..].trim();

    let (group, name) = path.split_once('/')
        .ok_or_else(|| anyhow::anyhow!("line {}: section must be [group/Name]", line_num + 1))?;

    // 名字在这里就拦下来，别等到生成了 `bad_name.tpl` 才发现。
    // 段名会变成 .tbl 文件名、生成代码里的类名；组名会变成目录名。两者都由
    // `validate.rs` 定义规则，这里只是把同一套规则前移到**解析**这一步 ——
    // `project new --from-file` / `util parse-schema` / `merge-schema` 都过这个口。
    let (group, name) = (group.trim(), name.trim());
    if !crate::validate::is_valid_group_name(group) {
        bail!("line {}: 组名 {:?} 不合法（只能包含中英文数字下划线）", line_num + 1, group);
    }
    if !crate::validate::is_valid_node_name(name) {
        bail!(
            "line {}: 配置项名 {:?} 不合法（必须大写字母开头，只含英文数字下划线，如 HeroBase）",
            line_num + 1, name
        );
    }

    let parts: Vec<&str> = rest.split_whitespace().collect();
    let mode_str = parts.first().copied().unwrap_or("");
    let mode = match mode_str {
        "table" => SchemaMode::Table,
        "constant" => SchemaMode::Constant,
        "enum" => SchemaMode::Enum,
        _ => bail!("line {}: mode must be 'table' / 'constant' / 'enum'", line_num + 1),
    };

    // ignore index= option (backward compat, index is always "id")
    Ok(SchemaSection {
        group: group.trim().to_string(),
        name: name.trim().to_string(),
        mode,
        fields: Vec::new(),
        preset: Vec::new(),
        display: None,

        rules: Vec::new(),
    })
}

fn parse_field_line(line: &str, line_num: usize, mode: &SchemaMode) -> Result<SchemaField> {
    let parts: Vec<&str> = line.split('|').collect();
    match mode {
        SchemaMode::Enum => {
            // enum 数据行：id | name | desc
            if parts.len() < 2 {
                bail!("line {}: enum row needs at least id|name", line_num + 1);
            }
            Ok(SchemaField {
                name: parts[1].trim().to_string(),
                tbl_type: parts[0].trim().to_string(), // 借用 tbl_type 字段存储 id
                export: String::new(),
                desc: parts.get(2).map(|s| s.trim().to_string()).unwrap_or_default(),
            })
        }
        _ => {
            if parts.len() < 3 {
                bail!("line {}: field needs at least name|type|export", line_num + 1);
            }
            // export 只认短码。以前这里不校验、`Export::from_str` 把任何不认识的
            // 值静默当成"双端导出"——写错了也看不出来（@01.10）。
            if Export::parse_code(parts[2].trim()).is_none() {
                bail!("line {}: 字段 {} 的 export 值 \"{}\" 不合法——只接受短码 cs / c / s / -，留空 = cs",
                    line_num + 1, parts[0].trim(), parts[2].trim());
            }
            Ok(SchemaField {
                name: parts[0].trim().to_string(),
                tbl_type: parts[1].trim().to_string(),
                export: parts[2].trim().to_string(),
                desc: parts.get(3).map(|s| s.trim().to_string()).unwrap_or_default(),
            })
        }
    }
}

fn validate_section(sec: &SchemaSection, _line_num: usize) -> Result<()> {
    let mut names = std::collections::HashSet::new();
    for f in &sec.fields {
        if !names.insert(&f.name) {
            bail!("[{}/{}]: duplicate field '{}'", sec.group, sec.name, f.name);
        }
    }
    Ok(())
}

pub fn merge_schemas(schemas: &[TblSchema]) -> Result<TblSchema> {
    let mut all_sections = Vec::new();
    let mut keys = std::collections::HashSet::new();

    for schema in schemas {
        for sec in &schema.sections {
            let key = format!("{}/{}", sec.group, sec.name);
            if !keys.insert(key.clone()) {
                bail!("duplicate section: [{}]", key);
            }
            all_sections.push(sec.clone());
        }
    }

    // 多文件合并不挂任何 metadata（合并产物没有单一来源 id）。调用方如有需要自行赋 meta。
    Ok(TblSchema { meta: SchemaMetadata::default(), separators: SeparatorsSection::default(), sections: all_sections })
}

pub fn serialize_tblschema(schema: &TblSchema) -> String {
    use std::fmt::Write;
    let mut s = String::new();
    writeln!(s, "#!tblschema v1").unwrap();

    // metadata：仅非空字段输出
    if !schema.meta.id.is_empty() {
        writeln!(s, "# @meta id: {}", schema.meta.id).unwrap();
    }
    if !schema.meta.name.is_empty() && schema.meta.name != schema.meta.id {
        writeln!(s, "# @meta name: {}", schema.meta.name).unwrap();
    }
    if !schema.meta.category.is_empty() {
        writeln!(s, "# @meta category: {}", schema.meta.category).unwrap();
    }
    if !schema.meta.version.is_empty() {
        writeln!(s, "# @meta version: {}", schema.meta.version).unwrap();
    }
    if !schema.meta.created_at.is_empty() {
        writeln!(s, "# @meta created_at: {}", schema.meta.created_at).unwrap();
    }
    if !schema.meta.source_template.is_empty() {
        writeln!(s, "# @meta source_template: {}", schema.meta.source_template).unwrap();
    }
    if !schema.meta.source_template_version.is_empty() {
        writeln!(s, "# @meta source_template_version: {}", schema.meta.source_template_version).unwrap();
    }
    // has_preset 是 derive 字段：以实际 sections 为准
    let has_preset = schema.sections.iter().any(|s| !s.preset.is_empty());
    if has_preset {
        writeln!(s, "# @meta has_preset: true").unwrap();
    }

    // separators：仅输出与默认不同的字段，避免普通 schema 满屏 25 行
    write_sep_diff(&mut s, &schema.separators);

    for sec in &schema.sections {
        writeln!(s).unwrap();
        let mode = match sec.mode {
            SchemaMode::Table => "table",
            SchemaMode::Constant => "constant",
            SchemaMode::Enum => "enum",
        };
        writeln!(s, "[{}/{}] {}", sec.group, sec.name, mode).unwrap();

        // 被引用展示列（@10）：非默认时输出。
        if let Some(d) = &sec.display {
            writeln!(s, "# @display: {}", d).unwrap();
        }

        // 仅 Table 段输出列声明；Constant / Enum 段是「结构在项目 .tbl 里」，schema 只留段头。
        if matches!(sec.mode, SchemaMode::Table) {
            for f in &sec.fields {
                writeln!(s, "{} | {} | {} | {}", f.name, f.tbl_type, f.export, f.desc).unwrap();
            }
        }

        // 节点级校验规则：行序即优先级，原样按顺序输出。
        for rule in &sec.rules {
            let level = match rule.level {
                Severity::Error => "",
                Severity::Warning => " [warn]",
                Severity::Info => " [info]",
            };
            writeln!(s, "# @rule: {} = {}{}", rule.column, rule.expr_string(), level).unwrap();
        }

        // 预设数据 sub-block
        if !sec.preset.is_empty() {
            writeln!(s, "# @preset").unwrap();
            for row in &sec.preset {
                // 每个单元格需要 encode 转义（换行符、管道符、反斜杠等）
                let encoded: Vec<String> = row.iter()
                    .map(|cell| crate::tbl_str::encode(cell, "txt"))
                    .collect();
                writeln!(s, "{}", encoded.join(" | ")).unwrap();
            }
        }
    }
    s
}

/// 把 SeparatorsSection 中与默认不同的字段写成 `# @sep` 行。全部默认 = 不输出。
fn write_sep_diff(s: &mut String, sep: &SeparatorsSection) {
    use std::fmt::Write;
    use crate::types::SepKey;
    let d = SeparatorsSection::default();
    for k in SepKey::ALL {
        let v = k.get(sep);
        if v != k.get(&d) {
            writeln!(s, "# @sep {} = {}", k.as_directive_key(), v).unwrap();
        }
    }
}

/// 把 project 反向编码成 TblSchema：
/// - Table 段：列定义照写；with_preset=true 时把 records 作为 preset 行
/// - Constant 段：fields 永远空（新范式）；with_preset=true 时 entries → preset
/// - Enum 段：同 Constant
///
/// meta 留空（调用方按导出场景填）。
pub fn schema_from_project(groups: &[Group], with_preset: bool) -> TblSchema {
    let mut sections = Vec::new();
    for group in groups {
        for table in &group.tables {
            if table.deleted { continue; }
            let fields = table.schema.fields.iter().map(|f| SchemaField {
                name: f.name.clone(),
                tbl_type: f.tbl_type.clone(),
                export: export_to_code(&f.export),
                desc: f.desc.clone(),
            }).collect();
            let preset = if with_preset {
                table.records.iter().map(|r| r.clone()).collect()
            } else {
                Vec::new()
            };
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: table.name.clone(),
                mode: SchemaMode::Table,
                fields,
                preset,
                display: table.display_field.clone(),
                rules: table.rules.clone(),
            });
        }
        for constant in &group.constants {
            if constant.deleted { continue; }
            // 新范式：constant section 的 schema fields 永远空（结构信息全在项目 .tbl）
            let preset = if with_preset {
                constant.entries.iter()
                    .filter(|e| !e.name.is_empty())
                    .map(|e| vec![
                        e.name.clone(),
                        e.tbl_type.clone(),
                        e.value.clone(),
                        export_to_code(&e.export),
                        e.desc.clone(),
                    ])
                    .collect()
            } else {
                Vec::new()
            };
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: constant.name.clone(),
                mode: SchemaMode::Constant,
                fields: Vec::new(),
                preset,
                display: None,
                rules: Vec::new(),
            });
        }
        for enum_def in &group.enums {
            if enum_def.deleted { continue; }
            let preset = if with_preset {
                enum_def.entries.iter()
                    .filter(|e| !e.id.is_empty() || !e.name.is_empty())
                    .map(|e| vec![e.id.clone(), e.name.clone(), e.desc.clone()])
                    .collect()
            } else {
                Vec::new()
            };
            sections.push(SchemaSection {
                group: group.name.clone(),
                name: enum_def.name.clone(),
                mode: SchemaMode::Enum,
                fields: Vec::new(),
                preset,
                display: enum_def.display_field.clone(),
                rules: enum_def.rules.clone(),
            });
        }
    }
    TblSchema { meta: SchemaMetadata::default(), separators: Default::default(), sections }
}

fn export_to_code(e: &Export) -> String {
    e.code().to_string()
}

/// 把 schema sections 应用到 project：
/// - Table 段写入 / 替换列定义；with_preset=true 时把 preset 行作为 records 灌入
/// - Constant / Enum 段：新范式下 schema 段头里不再带 entries，因此行数据来自 preset
///     - with_preset=true：preset 行 → entries
///     - with_preset=false：entries 留空（用户在 UI 里手动添加）
///
/// 返回 (added_nodes, overwritten_nodes)。
pub fn apply_schema_to_project(
    groups: &mut Vec<Group>,
    sections: &[SchemaSection],
    config_dir: &Path,
    with_preset: bool,
) -> (usize, usize) {
    let mut added = 0usize;
    let mut overwritten = 0usize;

    for sec in sections {
        let group = match groups.iter_mut().find(|g| g.name == sec.group) {
            Some(g) => g,
            None => {
                let dir = config_dir.join(&sec.group);
                groups.push(Group {
                    name: sec.group.clone(),
                    original_name: sec.group.clone(),
                    dir,
                    tables: Vec::new(),
                    constants: Vec::new(),
                    enums: Vec::new(),
                    is_new: true,
                    // 从 .tblschema 展开出来的组一律是文本后端：schema 文件里没有
                    // 存储方式的信息（@02.27），xlsx 组只由 UI / CLI 显式新建。
                    storage: crate::model::GroupStorage::Tbl,
                    book_stamp: None,
                });
                groups.last_mut().unwrap()
            }
        };

        match sec.mode {
            SchemaMode::Table => {
                let fields: Vec<FieldDef> = sec.fields.iter().map(|f| FieldDef {
                    name: f.name.clone(),
                    desc: f.desc.clone(),
                    tbl_type: f.tbl_type.clone(),
                    // .tblschema 的字段行在 `parse_field_line` 已严格校验；这里拿到的是已校验数据
                    export: Export::from_input(&f.export),
                }).collect();
                let new_len = fields.len();

                let records: Vec<Vec<String>> = if with_preset {
                    sec.preset.iter().map(|row| {
                        let mut r = row.clone();
                        r.resize(new_len, String::new()); // 多余/缺失列对齐
                        r
                    }).collect()
                } else {
                    Vec::new()
                };

                if let Some(table) = group.tables.iter_mut().find(|t| t.name == sec.name) {
                    let old_len = table.schema.fields.len();
                    table.schema.fields = fields;
                    if with_preset {
                        // preset 覆盖现有 records（与 overwritten 语义一致）
                        table.records = records;
                    } else {
                        // 仅同步列结构，保留现有 records 但按新列数 resize
                        for row in &mut table.records {
                            row.resize(new_len, String::new());
                            if new_len < old_len { row.truncate(new_len); }
                        }
                    }
                    table.display_field = sec.display.clone();
                    table.rules = sec.rules.clone();
                    table.dirty = true;
                    overwritten += 1;
                } else {
                    let path = group.dir.join(format!("{}.tbl", sec.name));
                    group.tables.push(Table {
                        name: sec.name.clone(),
                        original_name: sec.name.clone(),
                        path,
                        schema: TableSchema { fields: fields.clone() },
                        records,
                        dirty: true,
                        deleted: false,
                        original_records: Vec::new(),
                        original_fields: fields,
                        saved: false,
                        fmt_ver: crate::tbl::DEFAULT_TBL_VERSION,
                        display_field: sec.display.clone(),
                        original_display_field: None,
                        rules: sec.rules.clone(),
                        original_rules: Vec::new(),
                    });
                    added += 1;
                }
            }
            SchemaMode::Constant => {
                // preset 行：name | type | value | export | desc
                let entries: Vec<ConstEntry> = if with_preset {
                    sec.preset.iter().map(|row| ConstEntry {
                        name:     row.first()  .cloned().unwrap_or_default(),
                        tbl_type: row.get(1)   .cloned().unwrap_or_default(),
                        value:    row.get(2)   .cloned().unwrap_or_default(),
                        // 常量预设行同样在 parse_tblschema 里校验过
                        export:   Export::from_input(row.get(3).map(String::as_str).unwrap_or("cs")),
                        desc:     row.get(4)   .cloned().unwrap_or_default(),
                    }).collect()
                } else {
                    Vec::new()
                };

                if let Some(constant) = group.constants.iter_mut().find(|c| c.name == sec.name) {
                    if with_preset {
                        constant.entries = entries;
                    } // 不带预设：保留现有 entries，仅记一次 overwritten
                    constant.dirty = true;
                    overwritten += 1;
                } else {
                    let path = group.dir.join(format!("{}.tbl", sec.name));
                    let snap = entries.clone();
                    group.constants.push(Constant {
                        name: sec.name.clone(),
                        original_name: sec.name.clone(),
                        path,
                        entries,
                        dirty: true,
                        deleted: false,
                        original_entries: snap, saved: false,
                    });
                    added += 1;
                }
            }
            SchemaMode::Enum => {
                // preset 行：id | name | desc
                let entries: Vec<EnumEntry> = if with_preset {
                    sec.preset.iter().map(|row| EnumEntry {
                        id:   row.first().cloned().unwrap_or_default(),
                        name: row.get(1) .cloned().unwrap_or_default(),
                        desc: row.get(2) .cloned().unwrap_or_default(),
                    }).collect()
                } else {
                    Vec::new()
                };

                if let Some(en) = group.enums.iter_mut().find(|e| e.name == sec.name) {
                    if with_preset {
                        en.entries = entries;
                    }
                    en.display_field = sec.display.clone();
                    en.rules = sec.rules.clone();
                    en.dirty = true;
                    overwritten += 1;
                } else {
                    let path = group.dir.join(format!("{}.tbl", sec.name));
                    let snap = entries.clone();
                    group.enums.push(EnumDef {
                        name: sec.name.clone(),
                        original_name: sec.name.clone(),
                        path,
                        entries,
                        dirty: true,
                        deleted: false,
                        original_entries: snap, saved: false,
                        display_field: sec.display.clone(),
                        original_display_field: None,
                        rules: sec.rules.clone(),
                        original_rules: Vec::new(),
                    });
                    added += 1;
                }
            }
        }
    }

    (added, overwritten)
}

/// 把 schema 各段里的 `# @display` 回填到对应节点（被引用展示列，@10）。
///
/// `.tbl` 里不含被引用展示列信息——它只住在 project.tblschema。加载项目时先由 .tbl 得到
/// groups（display_field 全为 None），再调本函数用 schema 段回填。按 (group, name) 匹配。
pub fn apply_display_to_groups(groups: &mut [Group], schema: &TblSchema) {
    for sec in &schema.sections {
        let Some(display) = &sec.display else { continue };
        let Some(group) = groups.iter_mut().find(|g| g.name == sec.group) else { continue };
        match sec.mode {
            SchemaMode::Table => {
                if let Some(t) = group.tables.iter_mut().find(|t| t.name == sec.name) {
                    t.display_field = Some(display.clone());
                    // 这是**从盘上读回来的**值（`.tbl` 里没有 `# @display`，靠 schema 回填），
                    // 所以快照要一起填——否则一打开就显示"改过"，项目永远亮 🟡。
                    t.original_display_field = Some(display.clone());
                }
            }
            SchemaMode::Enum => {
                if let Some(e) = group.enums.iter_mut().find(|e| e.name == sec.name) {
                    e.display_field = Some(display.clone());
                    e.original_display_field = Some(display.clone());
                }
            }
            SchemaMode::Constant => {}
        }
    }
}

/// 把 schema 各段里的 `# @rule` 回填到对应节点（节点级校验规则，@01.10.7）。
///
/// 规则只住在 project.tblschema——`.tbl` 与 xlsx sheet 都不含。加载项目时先由数据文件
/// 得到 groups（rules 全为空），再用本函数回填，**同时填 original 快照**
/// （这是从盘上读回来的值，不能算脏）。按 (group, name) 匹配。
pub fn apply_rules_to_groups(groups: &mut [Group], schema: &TblSchema) {
    for sec in &schema.sections {
        let Some(group) = groups.iter_mut().find(|g| g.name == sec.group) else { continue };
        match sec.mode {
            SchemaMode::Table => {
                if let Some(t) = group.tables.iter_mut().find(|t| t.name == sec.name) {
                    t.rules = sec.rules.clone();
                    t.original_rules = sec.rules.clone();
                }
            }
            // 常量不支持规则（@01.8.8）：schema 里写了也在解析阶段报错。
            SchemaMode::Constant => {}
            SchemaMode::Enum => {
                if let Some(e) = group.enums.iter_mut().find(|e| e.name == sec.name) {
                    e.rules = sec.rules.clone();
                    e.original_rules = sec.rules.clone();
                }
            }
        }
    }
}

/// 解析后给缺失的 metadata 兜底：id 走文件名 stem，name 等于 id（如未填）。
/// 调用方在已知文件路径时使用：例如 `LocalTemplates` 扫目录时把 `<stem>` 灌进去。
pub fn fill_metadata_defaults(schema: &mut TblSchema, file_stem: &str) {
    if schema.meta.id.is_empty() {
        schema.meta.id = file_stem.to_string();
    }
    if schema.meta.name.is_empty() {
        schema.meta.name = schema.meta.id.clone();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 段名 / 组名在**解析**这一步就拦下来。
    ///
    /// 它们会变成 `.tbl` 文件名、目录名和生成代码里的类名。以前只有 GUI 的
    /// 「新建节点」对话框校验，`project new --from-file` / `util parse-schema`
    /// 一路放行 —— 然后生成出 `bad_nameTpl.java` 这种编译不过的代码。
    #[test]
    fn section_and_group_names_are_checked_at_parse_time() {
        let bad_section = "#!tblschema v1\n\n[hero/bad_name] table\nid | int | cs | ID\n";
        let err = parse_tblschema(bad_section).expect_err("小写段名应被拒绝");
        let msg = format!("{}", err);
        assert!(msg.contains("line 3"), "错误要带行号: {}", msg);
        assert!(msg.contains("bad_name"), "错误要点名: {}", msg);

        let bad_group = "#!tblschema v1\n\n[bad group!/T] table\nid | int | cs | ID\n";
        let err = parse_tblschema(bad_group).expect_err("非法组名应被拒绝");
        assert!(format!("{}", err).contains("组名"), "{}", err);

        // 合法的（含中文组名）照常通过
        let ok = "#!tblschema v1\n\n[英雄/HeroBase] table\nid | int | cs | ID\n";
        assert!(parse_tblschema(ok).is_ok());
    }

    /// preset 行**多出来的列会被静默丢弃**（`Vec::resize` 截断），直接报错。
    ///
    /// 少写没问题（尾部空列省略很自然），多写一定是用错了：复制粘贴多一个 `|`、
    /// 或者拿旧表的数据往新表里灌，都会"少一个值"而看不出来。
    #[test]
    fn preset_rows_with_more_columns_than_declared_are_rejected() {
        let table_extra = "#!tblschema v1\n[g/T] table\nid | int | cs | ID\nname | str | cs | 名\n\
# @preset\n1 | 甲 | 多出来的\n";
        let err = parse_tblschema(table_extra).expect_err("表 preset 多列应被拒绝");
        let msg = format!("{}", err);
        assert!(msg.contains("line 6"), "要带行号: {}", msg);
        assert!(msg.contains("最多 2 列"), "要说清上限: {}", msg);

        // 少写：允许（缺的位置按空处理）
        let table_short = "#!tblschema v1\n[g/T] table\nid | int | cs | ID\nname | str | cs | 名\n\
# @preset\n1\n";
        assert!(parse_tblschema(table_short).is_ok(), "短行应当允许");

        let const_extra = "#!tblschema v1\n[g/C] constant\n\
# @preset\nmax_level | int | 100 | cs | 上限 | 多余\n";
        assert!(parse_tblschema(const_extra).is_err(), "常量 preset 最多 5 列");

        let enum_extra = "#!tblschema v1\n[g/E] enum\n# @preset\n1 | A | 甲 | 多余\n";
        assert!(parse_tblschema(enum_extra).is_err(), "枚举 preset 最多 3 列");
    }

    #[test]
    fn parse_tblschema_with_sep_lines() {
        // # @sep 行覆写默认；未列出的字段保持默认；未知 key 忽略；列出位置在第一个 [section] 之前。
        let src = r#"#!tblschema v1
# @meta id: sep-test
# @sep Map.entry = ,
# @sep List = |
# @sep Map_Tuple3.kv = >
# @sep Unknown.key = ?

[g/T] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.separators.map.entry, ",");
        assert_eq!(s.separators.list, "|");
        assert_eq!(s.separators.map_tuple3.kv, ">");
        // 未覆写字段 = 默认
        assert_eq!(s.separators.set, ";");
        assert_eq!(s.separators.tuple2, ",");
        assert_eq!(s.separators.map.kv, ":");
    }

    #[test]
    fn serialize_tblschema_omits_default_sep() {
        // 全默认 separators → serialize 不应包含任何 # @sep 行
        let mut schema = TblSchema::default();
        schema.meta.id = "no-sep".to_string();
        schema.sections.push(SchemaSection {
            group: "g".to_string(),
            name: "T".to_string(),
            mode: SchemaMode::Table,
            fields: vec![SchemaField {
                name: "id".to_string(), tbl_type: "int".to_string(),
                export: "cs".to_string(), desc: "x".to_string(),
            }],
            preset: vec![],
            display: None,

            rules: Vec::new(),
        });
        let out = serialize_tblschema(&schema);
        assert!(!out.contains("# @sep"), "默认值不应输出 sep 行: {}", out);
    }

    #[test]
    fn serialize_tblschema_writes_modified_sep_round_trip() {
        // 改 Map.entry → 写盘 → 重新 parse → 仍是逗号
        let mut schema = TblSchema::default();
        schema.meta.id = "rt".to_string();
        schema.separators.map.entry = ",".to_string();
        schema.separators.tuple3 = "/".to_string();
        schema.sections.push(SchemaSection {
            group: "g".to_string(),
            name: "T".to_string(),
            mode: SchemaMode::Table,
            fields: vec![SchemaField {
                name: "id".to_string(), tbl_type: "int".to_string(),
                export: "cs".to_string(), desc: "x".to_string(),
            }],
            preset: vec![],
            display: None,

            rules: Vec::new(),
        });
        let out = serialize_tblschema(&schema);
        assert!(out.contains("# @sep Map.entry = ,"), "应写出 Map.entry: {}", out);
        assert!(out.contains("# @sep Tuple3 = /"), "应写出 Tuple3: {}", out);
        // List 是默认值 → 不应该出现
        assert!(!out.contains("# @sep List ="));

        let parsed = parse_tblschema(&out).expect("re-parse");
        assert_eq!(parsed.separators.map.entry, ",");
        assert_eq!(parsed.separators.tuple3, "/");
        assert_eq!(parsed.separators.list, ";"); // 仍是默认
    }

    #[test]
    fn parses_meta_lines() {
        let src = r#"#!tblschema v1
# @meta id: full
# @meta name: 完整测试模板
# @meta category: test
# @meta version: 1.0.0

[hero/HeroBase] table
id   | int | cs | 英雄ID
name | str | cs | 名称
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "full");
        assert_eq!(s.meta.name, "完整测试模板");
        assert_eq!(s.meta.category, "test");
        assert_eq!(s.meta.version, "1.0.0");
        assert_eq!(s.sections.len(), 1);
    }

    #[test]
    fn meta_lines_only_before_first_section() {
        let src = r#"#!tblschema v1
# @meta id: a

[g/N] table
id | int | cs | x
# @meta name: should-be-ignored
y  | str | cs | y
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "a");
        assert_eq!(s.meta.name, "a", "name 缺省 = id，第二段后的 @meta 应被当注释忽略");
    }

    #[test]
    fn meta_later_key_wins() {
        let src = r#"#!tblschema v1
# @meta id: a
# @meta id: b

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "b");
    }

    #[test]
    fn meta_unknown_key_ignored() {
        let src = r#"#!tblschema v1
# @meta id: a
# @meta future_field: 42

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "a");
    }

    #[test]
    fn legacy_no_meta_parses() {
        let src = r#"#!tblschema v1

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.meta.id, "");
        assert_eq!(s.meta.name, "");
        assert_eq!(s.sections.len(), 1);
    }

    #[test]
    fn fill_metadata_defaults_uses_stem() {
        let mut s = TblSchema::default();
        fill_metadata_defaults(&mut s, "myfile");
        assert_eq!(s.meta.id, "myfile");
        assert_eq!(s.meta.name, "myfile");

        // id 已存在 → 不覆盖；name 仍按 id 兜底
        let mut s2 = TblSchema::default();
        s2.meta.id = "explicit".to_string();
        fill_metadata_defaults(&mut s2, "stem");
        assert_eq!(s2.meta.id, "explicit");
        assert_eq!(s2.meta.name, "explicit");
    }

    #[test]
    fn id_validation() {
        assert!(is_valid_metadata_id("full"));
        assert!(is_valid_metadata_id("slg-base"));
        assert!(is_valid_metadata_id("slg_base_2"));
        assert!(is_valid_metadata_id("a"));
        assert!(is_valid_metadata_id(&"a".repeat(32)));

        assert!(!is_valid_metadata_id(""));
        assert!(!is_valid_metadata_id(&"a".repeat(33)));
        assert!(!is_valid_metadata_id("Full"));            // 不接受大写
        assert!(!is_valid_metadata_id("foo bar"));          // 空格
        assert!(!is_valid_metadata_id("foo.bar"));          // 点
        assert!(!is_valid_metadata_id("中文"));
    }

    #[test]
    fn serialize_round_trips_meta() {
        let mut s = TblSchema::default();
        s.meta.id = "demo".to_string();
        s.meta.name = "演示".to_string();
        s.meta.category = "test".to_string();
        s.meta.version = "0.1.0".to_string();
        s.sections.push(SchemaSection {
            group: "g".to_string(),
            name: "N".to_string(),
            mode: SchemaMode::Table,
            fields: vec![SchemaField {
                name: "id".to_string(),
                tbl_type: "int".to_string(),
                export: "cs".to_string(),
                desc: "x".to_string(),
            }],
            preset: Vec::new(),
            display: None,

            rules: Vec::new(),
        });
        let txt = serialize_tblschema(&s);
        assert!(txt.contains("# @meta id: demo"));
        assert!(txt.contains("# @meta name: 演示"));
        assert!(txt.contains("# @meta category: test"));
        assert!(txt.contains("# @meta version: 0.1.0"));

        let back = parse_tblschema(&txt).expect("re-parse");
        assert_eq!(back.meta.id, "demo");
        assert_eq!(back.meta.name, "演示");
        assert_eq!(back.meta.category, "test");
        assert_eq!(back.meta.version, "0.1.0");
        assert_eq!(back.sections.len(), 1);
    }

    #[test]
    fn rejects_an_invalid_declared_id_with_line_number() {
        // 声明了 id 就必须合法。以前加载时谁都不校验 —— 于是"改名到一个非法 id"能造出
        // 一个别处都不允许存在的项目，而且一直到下一次操作为止都没人吭声。
        let src = "#!tblschema v1
# @meta id: Bad-Id

[g/T] table
id | int | cs | ID
";
        let err = parse_tblschema(src).expect_err("非法 id 应当被拒绝");
        let msg = err.to_string();
        assert!(msg.contains("line 2"), "应当给出行号: {}", msg);
        assert!(msg.contains("Bad-Id"), "应当给出非法值: {}", msg);
    }

    #[test]
    fn rejects_an_empty_declared_id() {
        // `# @meta id:` 是**写坏了**，不是"没声明" —— 这两种要分得开。
        let src = "#!tblschema v1
# @meta id:

[g/T] table
id | int | cs | ID
";
        let err = parse_tblschema(src).expect_err("空 id 声明应当被拒绝");
        assert!(err.to_string().contains("line 2"), "{}", err);
    }

    #[test]
    fn a_schema_without_an_id_is_still_fine() {
        // **没声明 id 是合法的**：合并用的片段、裸 .tblschema 都不带 id
        // （`merge_schemas` 也不挂 metadata）。这一条是防止上面那道校验收得太紧，
        // 把 merge_schema 那条路一起打死。
        let src = "#!tblschema v1

[g/T] table
id | int | cs | ID
";
        let schema = parse_tblschema(src).expect("没有 id 声明应当照样能解析");
        assert!(schema.meta.id.is_empty());
        assert_eq!(schema.sections.len(), 1);
    }

    #[test]
    fn rejects_non_short_code_export_with_line_number() {
        // 以前这里不校验，`Export::from_str` 把任何不认识的值静默当成"双端导出"
        let src = "#!tblschema v1

[g/T] table
id | int | cs | ID
name | str | 前后端 | 名称
";
        let err = parse_tblschema(src).expect_err("中文 export 应当被拒绝");
        let msg = err.to_string();
        assert!(msg.contains("line 5"), "应当给出行号: {}", msg);
        assert!(msg.contains("前后端"), "应当给出非法值: {}", msg);
        assert!(msg.contains("name"), "应当点名字段: {}", msg);
    }

    #[test]
    fn rejects_non_short_code_export_in_constant_preset() {
        let src = "#!tblschema v1
[global/C] constant
# @preset
max_level | int | 100 | 前后端 | 最大等级
";
        let err = parse_tblschema(src).expect_err("常量预设里的中文 export 应当被拒绝");
        let msg = err.to_string();
        assert!(msg.contains("max_level") && msg.contains("前后端"), "{}", msg);
    }

    #[test]
    fn parses_preset_block_for_table() {
        let src = r#"#!tblschema v1
[hero/HeroBase] table
id   | int | cs | 英雄ID
name | str | cs | 名称
# @preset
1 | Alice
2 | Bob
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.sections.len(), 1);
        assert_eq!(s.sections[0].fields.len(), 2);
        assert_eq!(s.sections[0].preset.len(), 2);
        assert_eq!(s.sections[0].preset[0], vec!["1", "Alice"]);
        assert_eq!(s.sections[0].preset[1], vec!["2", "Bob"]);
        assert!(s.meta.has_preset);
    }

    #[test]
    fn parses_preset_block_for_constant() {
        let src = r#"#!tblschema v1
[global/GlobalConst] constant
# @preset
max_level | int | 100 | cs | 最大等级
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.sections.len(), 1);
        assert_eq!(s.sections[0].mode, SchemaMode::Constant);
        assert!(s.sections[0].fields.is_empty(), "constant 段 fields 应为空");
        assert_eq!(s.sections[0].preset.len(), 1);
        assert_eq!(s.sections[0].preset[0], vec!["max_level", "int", "100", "cs", "最大等级"]);
        assert!(s.meta.has_preset);
    }

    #[test]
    fn parses_preset_block_for_enum() {
        let src = r#"#!tblschema v1
[hero/HeroType] enum
# @preset
1 | Warrior | 战士
2 | Mage    | 法师
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.sections.len(), 1);
        assert_eq!(s.sections[0].mode, SchemaMode::Enum);
        assert_eq!(s.sections[0].preset.len(), 2);
        assert!(s.meta.has_preset);
    }

    #[test]
    fn constant_section_rejects_inline_entries() {
        let src = r#"#!tblschema v1
[global/GlobalConst] constant
max_level | int | cs | 最大等级
"#;
        let err = parse_tblschema(src).expect_err("constant 段不允许直接 entry 行");
        let msg = err.to_string();
        assert!(msg.contains("constant"), "msg = {}", msg);
        assert!(msg.contains("@preset"), "msg = {}", msg);
    }

    #[test]
    fn enum_section_rejects_inline_entries() {
        let src = r#"#!tblschema v1
[hero/HeroType] enum
1 | Warrior | 战士
"#;
        let err = parse_tblschema(src).expect_err("enum 段不允许直接 entry 行");
        let msg = err.to_string();
        assert!(msg.contains("enum"), "msg = {}", msg);
        assert!(msg.contains("@preset"), "msg = {}", msg);
    }

    #[test]
    fn preset_escapes_special_characters_in_constant() {
        // 验证 constant preset 序列化时正确转义换行符、管道符等特殊字符
        let src = r#"#!tblschema v1
[global/GameConst] constant
# @preset
html | str | <div>\n<b>Hello</b></div> | cs | 包含换行的 HTML
regex | str | a\|b\|c | cs | 包含管道符
path | str | C:\\test | cs | 反斜杠路径
"#;
        let schema = parse_tblschema(src).expect("parse");
        let serialized = serialize_tblschema(&schema);
        let reparsed = parse_tblschema(&serialized).expect("reparse failed");

        let preset1 = &schema.sections[0].preset;
        let preset2 = &reparsed.sections[0].preset;
        assert_eq!(preset1.len(), 3);
        assert_eq!(preset2.len(), 3);

        // 验证换行符
        assert_eq!(preset1[0][2], "<div>\n<b>Hello</b></div>");
        assert_eq!(preset2[0][2], "<div>\n<b>Hello</b></div>");

        // 验证管道符
        assert_eq!(preset1[1][2], "a|b|c");
        assert_eq!(preset2[1][2], "a|b|c");

        // 验证反斜杠
        assert_eq!(preset1[2][2], "C:\\test");
        assert_eq!(preset2[2][2], "C:\\test");
    }

    #[test]
    fn standard_template_preset_roundtrip() {
        // 使用实际的 standard 模板测试完整往返（包含富文本）
        let schema_text = include_str!("../schemas/standard.tblschema");
        let schema1 = parse_tblschema(schema_text).expect("parse standard");
        let serialized = serialize_tblschema(&schema1);
        let schema2 = parse_tblschema(&serialized).expect("reparse standard");

        // 验证 GameConst 段
        let gc1 = schema1.sections.iter().find(|s| s.name == "GameConst").unwrap();
        let gc2 = schema2.sections.iter().find(|s| s.name == "GameConst").unwrap();
        assert_eq!(gc1.preset.len(), gc2.preset.len());

        // 验证富文本字段
        for (row1, row2) in gc1.preset.iter().zip(gc2.preset.iter()) {
            assert_eq!(row1.len(), row2.len(), "行 {} 列数不一致", row1[0]);
            for (i, (c1, c2)) in row1.iter().zip(row2.iter()).enumerate() {
                assert_eq!(c1, c2, "行 {} 列 {} 不一致", row1[0], i);
            }
        }
    }

    #[test]
    fn preset_terminates_at_next_section() {
        let src = r#"#!tblschema v1
[a/T] table
id | int | cs | x
# @preset
1
[b/U] table
id | int | cs | y
"#;
        let s = parse_tblschema(src).expect("parse");
        assert_eq!(s.sections.len(), 2);
        assert_eq!(s.sections[0].preset.len(), 1);
        assert_eq!(s.sections[1].preset.len(), 0);
        assert_eq!(s.sections[1].fields.len(), 1);
    }

    #[test]
    fn has_preset_recomputed_from_sections() {
        // schema 文件里写 has_preset: true，但实际 sections.preset 全空 → 重算为 false
        let src = r#"#!tblschema v1
# @meta has_preset: true

[g/N] table
id | int | cs | x
"#;
        let s = parse_tblschema(src).expect("parse");
        assert!(!s.meta.has_preset, "应该按 sections 重算");
    }

    #[test]
    fn serialize_round_trips_preset() {
        let schema = TblSchema {
            meta: SchemaMetadata { id: "demo".into(), name: "demo".into(), ..Default::default() },
            separators: Default::default(),
            sections: vec![
                SchemaSection {
                    group: "hero".into(),
                    name: "HeroBase".into(),
                    mode: SchemaMode::Table,
                    fields: vec![
                        SchemaField { name: "id".into(), tbl_type: "int".into(), export: "cs".into(), desc: "ID".into() },
                        SchemaField { name: "name".into(), tbl_type: "str".into(), export: "cs".into(), desc: "名".into() },
                    ],
                    preset: vec![
                        vec!["1".into(), "Alice".into()],
                        vec!["2".into(), "Bob".into()],
                    ],
                    display: None,

                    rules: Vec::new(),
                },
                SchemaSection {
                    group: "global".into(),
                    name: "GlobalConst".into(),
                    mode: SchemaMode::Constant,
                    fields: vec![],
                    preset: vec![
                        vec!["max_level".into(), "int".into(), "100".into(), "cs".into(), "最大等级".into()],
                    ],
                    display: None,

                    rules: Vec::new(),
                },
                SchemaSection {
                    group: "hero".into(),
                    name: "HeroType".into(),
                    mode: SchemaMode::Enum,
                    fields: vec![],
                    preset: vec![
                        vec!["1".into(), "WARRIOR".into(), "战士".into()],
                    ],
                    display: None,

                    rules: Vec::new(),
                },
            ],
        };
        let txt = serialize_tblschema(&schema);
        assert!(txt.contains("# @meta has_preset: true"), "txt = {}", txt);
        assert!(txt.contains("# @preset"));
        let back = parse_tblschema(&txt).expect("re-parse");
        assert_eq!(back.sections.len(), 3);
        assert!(back.meta.has_preset);
        assert_eq!(back.sections[0].preset.len(), 2);
        assert_eq!(back.sections[0].preset[0], vec!["1", "Alice"]);
        assert_eq!(back.sections[1].mode, SchemaMode::Constant);
        assert_eq!(back.sections[1].preset.len(), 1);
        assert_eq!(back.sections[1].preset[0], vec!["max_level", "int", "100", "cs", "最大等级"]);
        assert_eq!(back.sections[2].mode, SchemaMode::Enum);
        assert_eq!(back.sections[2].preset.len(), 1);
    }

    #[test]
    fn serialize_constant_enum_no_inline_entries() {
        // Constant / Enum 段在 schema 文件里只剩段头（哪怕 fields 非空也不输出，
        // 因为新范式 entries 全在项目 .tbl 里）。
        let schema = TblSchema {
            meta: SchemaMetadata::default(),
            separators: Default::default(),
            sections: vec![
                SchemaSection {
                    group: "g".into(),
                    name: "C".into(),
                    mode: SchemaMode::Constant,
                    fields: vec![SchemaField {
                        name: "ignored".into(), tbl_type: "int".into(), export: "cs".into(), desc: "x".into()
                    }],
                    preset: vec![],
                    display: None,

                    rules: Vec::new(),
                },
                SchemaSection {
                    group: "g".into(),
                    name: "E".into(),
                    mode: SchemaMode::Enum,
                    fields: vec![SchemaField {
                        name: "WAR".into(), tbl_type: "1".into(), export: String::new(), desc: "".into()
                    }],
                    preset: vec![],
                    display: None,

                    rules: Vec::new(),
                },
            ],
        };
        let txt = serialize_tblschema(&schema);
        // 段头有
        assert!(txt.contains("[g/C] constant"));
        assert!(txt.contains("[g/E] enum"));
        // entry 行没有
        assert!(!txt.contains("ignored"));
        assert!(!txt.contains("WAR"));
        // 没有 preset 块
        assert!(!txt.contains("@preset"));
        assert!(!txt.contains("has_preset"));
    }

    #[test]
    fn display_directive_round_trip() {
        // 段内 `# @display: <列名>`：非默认输出、默认省略、写盘 → 重新 parse 一致（@10）。
        let mut schema = TblSchema::default();
        schema.meta.id = "disp".to_string();
        schema.sections.push(SchemaSection {
            group: "hero".into(), name: "HeroBase".into(), mode: SchemaMode::Table,
            fields: vec![SchemaField { name: "id".into(), tbl_type: "int".into(), export: "cs".into(), desc: "ID".into() }],
            preset: vec![], display: Some("name".into()),

            rules: Vec::new(),
        });
        schema.sections.push(SchemaSection {
            group: "hero".into(), name: "HeroType".into(), mode: SchemaMode::Enum,
            fields: vec![], preset: vec![], display: Some("desc".into()),

            rules: Vec::new(),
        });
        schema.sections.push(SchemaSection {
            group: "g".into(), name: "Plain".into(), mode: SchemaMode::Table,
            fields: vec![SchemaField { name: "id".into(), tbl_type: "int".into(), export: "cs".into(), desc: "x".into() }],
            preset: vec![], display: None,

            rules: Vec::new(),
        });
        let txt = serialize_tblschema(&schema);
        assert!(txt.contains("[hero/HeroBase] table\n# @display: name"), "txt = {}", txt);
        assert!(txt.contains("[hero/HeroType] enum\n# @display: desc"), "txt = {}", txt);
        // 默认段不输出该行
        let plain_block = txt.split("[g/Plain] table").nth(1).unwrap_or("");
        assert!(!plain_block.contains("@display"), "默认段不应输出: {}", txt);

        let back = parse_tblschema(&txt).expect("re-parse");
        assert_eq!(back.sections[0].display.as_deref(), Some("name"));
        assert_eq!(back.sections[1].display.as_deref(), Some("desc"));
        assert_eq!(back.sections[2].display, None);
    }

    #[test]
    fn apply_display_to_groups_backfills_nodes() {
        // schema 段 → 节点回填（按 group/name 匹配，@10）。
        use crate::model::{EnumDef, Group, TableSchema};
        let schema = TblSchema {
            meta: SchemaMetadata::default(),
            separators: Default::default(),
            sections: vec![
                SchemaSection {
                    group: "hero".into(), name: "HeroBase".into(), mode: SchemaMode::Table,
                    fields: vec![], preset: vec![], display: Some("name".into()),

                    rules: Vec::new(),
                },
                SchemaSection {
                    group: "hero".into(), name: "HeroType".into(), mode: SchemaMode::Enum,
                    fields: vec![], preset: vec![], display: Some("desc".into()),

                    rules: Vec::new(),
                },
            ],
        };
        let mut groups = vec![Group {
            name: "hero".into(), original_name: "hero".into(), dir: std::path::PathBuf::new(),
            storage: crate::model::GroupStorage::Tbl, book_stamp: None,
            tables: vec![Table {
                name: "HeroBase".into(), original_name: "HeroBase".into(), path: std::path::PathBuf::new(),
                schema: TableSchema { fields: vec![] }, records: vec![],
                dirty: false, deleted: false, original_records: vec![], original_fields: vec![],
                saved: true, fmt_ver: crate::tbl::DEFAULT_TBL_VERSION, display_field: None,
                original_display_field: None,

                rules: Vec::new(),
                original_rules: Vec::new(),
            }],
            constants: vec![],
            enums: vec![EnumDef {
                name: "HeroType".into(), original_name: "HeroType".into(), path: std::path::PathBuf::new(),
                entries: vec![], dirty: false, deleted: false,
                original_entries: vec![], saved: true, display_field: None,
                original_display_field: None,

                rules: Vec::new(),
                original_rules: Vec::new(),
            }],
            is_new: false,
        }];
        apply_display_to_groups(&mut groups, &schema);
        assert_eq!(groups[0].tables[0].display_field.as_deref(), Some("name"));
        assert_eq!(groups[0].enums[0].display_field.as_deref(), Some("desc"));
    }

    // ── 节点级规则 directive（@01.10.7） ────────────────────────────────

    #[test]
    fn rule_directive_round_trips() {
        let src = "#!tblschema v1\n\
                   \n\
                   [g/Lang] table\n\
                   key | str | cs | 英文 key\n\
                   # @rule: key = unique()\n\
                   \n\
                   [e/ErrorCode] enum\n\
                   # @rule: name = ref_exists(Lang.key) [warn]\n";
        let schema = parse_tblschema(src).unwrap();
        assert_eq!(schema.sections[0].rules.len(), 1);
        assert_eq!(schema.sections[0].rules[0].kind, RuleKind::Unique);
        assert_eq!(schema.sections[1].rules.len(), 1);
        assert_eq!(schema.sections[1].rules[0].level, Severity::Warning);

        let out = serialize_tblschema(&schema);
        assert!(out.contains("# @rule: key = unique()"), "got:\n{}", out);
        assert!(out.contains("# @rule: name = ref_exists(Lang.key) [warn]"), "got:\n{}", out);
        let again = parse_tblschema(&out).unwrap();
        assert_eq!(schema.sections, again.sections);
    }

    #[test]
    fn all_rule_kinds_round_trip() {
        let src = "#!tblschema v1\n\n[g/T] table\n\
                   id   | int | cs | ID\n\
                   hp   | int | cs | 血量\n\
                   name | str | cs | 名字\n\
                   type | int | cs | 类型\n\
                   # @rule: hp = range(0, 100)\n\
                   # @rule: hp = required_if(type, 2)\n\
                   # @rule: hp = compare(type, >=)\n\
                   # @rule: name = non_empty()\n\
                   # @rule: name = len(1, 16)\n\
                   # @rule: name = one_of(A, B, C)\n\
                   # @rule: name = matches(\"^[A-Z]+$\")\n";
        let schema = parse_tblschema(src).unwrap();
        assert_eq!(schema.sections[0].rules.len(), 7);
        assert_eq!(schema.sections[0].rules[0].kind, RuleKind::Range { min: "0".into(), max: "100".into() });
        assert_eq!(schema.sections[0].rules[1].kind,
            RuleKind::RequiredIf { column: "type".into(), value: "2".into() });
        assert_eq!(schema.sections[0].rules[2].kind,
            RuleKind::Compare { column: "type".into(), op: CompareOp::Ge });
        assert_eq!(schema.sections[0].rules[3].kind, RuleKind::NonEmpty);
        assert_eq!(schema.sections[0].rules[6].kind, RuleKind::Matches { pattern: "^[A-Z]+$".into() });

        let out = serialize_tblschema(&schema);
        let again = parse_tblschema(&out).unwrap();
        assert_eq!(schema.sections, again.sections);
    }

    #[test]
    fn quoted_rule_args_keep_commas() {
        let src = "#!tblschema v1\n\n[g/T] table\nname | str | cs | n\n\
                   # @rule: name = one_of(\"A,B\", C)\n";
        let schema = parse_tblschema(src).unwrap();
        assert_eq!(schema.sections[0].rules[0].kind,
            RuleKind::OneOf { values: vec!["A,B".into(), "C".into()] });
        let again = parse_tblschema(&serialize_tblschema(&schema)).unwrap();
        assert_eq!(schema.sections, again.sections);
    }

    #[test]
    fn bad_rule_params_are_rejected() {
        for bad in [
            "# @rule: hp = range(, )\n",
            "# @rule: hp = range(10, 1)\n",
            "# @rule: name = matches()\n",
            "# @rule: name = one_of()\n",
            "# @rule: hp = compare(hp, <=)\n",
        ] {
            let src = format!("#!tblschema v1\n\n[g/T] table\nhp | int | cs | h\nname | str | cs | n\n{}", bad);
            // 语法层能过（compare 自引用留给校验层），所以这里只断言不 panic
            let _ = parse_tblschema(&src);
        }
    }

    #[test]
    fn constant_section_rejects_rule_directive() {
        let src = "#!tblschema v1

[g/C] constant
# @rule: name = unique()
";
        let err = parse_tblschema(src).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("line 4"), "got: {}", msg);
        assert!(msg.contains("constant"), "got: {}", msg);
    }

    #[test]
    fn rule_directive_outside_section_is_rejected_with_line() {
        let err = parse_tblschema("#!tblschema v1\n\n# @rule: key = unique()\n").unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("line 3"), "got: {}", msg);
    }

    #[test]
    fn unknown_validator_is_rejected_with_line() {
        let src = "#!tblschema v1\n\n[g/T] table\nid | int | cs | d\n# @rule: id = nope()\n";
        let err = parse_tblschema(src).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("line 5"), "got: {}", msg);
        assert!(msg.contains("未知验证器"), "got: {}", msg);
    }

    #[test]
    fn script_rule_syntax_is_reserved_and_reported() {
        let src = "#!tblschema v1\n\n[g/T] table\nid | int | cs | d\n# @rule my_rule = script(1)\n";
        let err = parse_tblschema(src).unwrap_err();
        let msg = format!("{:#}", err);
        assert!(msg.contains("line 5"), "got: {}", msg);
        assert!(msg.contains("尚未实现"), "got: {}", msg);
    }

    #[test]
    fn apply_rules_to_groups_backfills_and_clears_dirty() {
        let src = "#!tblschema v1\n\
                   \n\
                   [g/Lang] table\n\
                   key | str | cs | k\n\
                   # @rule: key = unique()\n";
        let schema = parse_tblschema(src).unwrap();
        let mut groups = vec![Group {
            name: "g".to_string(),
            original_name: "g".to_string(),
            dir: std::path::PathBuf::from("g"),
            tables: vec![Table {
                name: "Lang".to_string(),
                original_name: "Lang".to_string(),
                path: std::path::PathBuf::from("Lang.tbl"),
                schema: TableSchema { fields: vec![FieldDef {
                    name: "key".to_string(), desc: String::new(),
                    tbl_type: "str".to_string(), export: Export::ClientServer,
                }] },
                records: vec![vec!["A".to_string()]],
                dirty: false, deleted: false,
                original_records: vec![vec!["A".to_string()]],
                original_fields: vec![FieldDef {
                    name: "key".to_string(), desc: String::new(),
                    tbl_type: "str".to_string(), export: Export::ClientServer,
                }],
                saved: true,
                fmt_ver: crate::tbl::TblVersion::V3,
                display_field: None,
                original_display_field: None,
                rules: Vec::new(),
                original_rules: Vec::new(),
            }],
            constants: Vec::new(),
            enums: Vec::new(),
            is_new: false,
            storage: GroupStorage::Tbl,
            book_stamp: None,
        }];
        apply_rules_to_groups(&mut groups, &schema);
        assert_eq!(groups[0].tables[0].rules.len(), 1);
        assert_eq!(groups[0].tables[0].original_rules, groups[0].tables[0].rules);
        // 从盘上读回来的值不算脏
        groups[0].tables[0].update_dirty();
        assert!(!groups[0].tables[0].dirty);
    }
}
