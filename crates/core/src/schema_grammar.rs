//! schema 文档的共享语法工具：meta/sep、section 头、rule/display 词法、写盘 helper。
//!
//! 这里**不做格式策略**：某个格式允不允许字段行 / preset / 某个版本，由对应格式模块决定。

use anyhow::{Result, bail};

use crate::rules::{CompareOp, Rule, RuleKind, RuleSource, Severity};
use crate::schema_model::*;
use crate::types::SeparatorsSection;

pub(crate) fn parse_meta_line(line: &str) -> Option<(String, String)> {
    // 去掉前导 '#'，再去空白
    let body = line.trim_start_matches('#').trim();
    let after = body.strip_prefix("@meta")?.trim_start();
    let (key, value) = after.split_once(':')?;
    Some((key.trim().to_string(), value.trim().to_string()))
}

pub(crate) fn parse_sep_line(line: &str) -> Option<(String, String)> {
    let body = line.trim_start_matches('#').trim();
    let after = body.strip_prefix("@sep")?.trim_start();
    let (key, value) = after.split_once('=')?;
    Some((key.trim().to_string(), value.trim_start().trim_end_matches('\n').to_string()))
}

pub(crate) fn parse_rule_body(body: &str) -> Result<Rule, String> {
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
    Ok(Rule { column: col.to_string(), kind, level, source: RuleSource::User })
}

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
        "positive_int" | "upper_snake" => Err(format!(
            "{}() 是系统内置规则（见 @01.8.11），不需要也不能在 schema 里声明", name
        )),
        _ => Err(format!(
            "未知验证器 \"{}\"（支持 unique / non_empty / ref_exists / range / len / one_of / matches / required_if / compare）",
            name
        )),
    }
}

pub(crate) fn apply_sep_kv(sep: &mut SeparatorsSection, key: &str, value: &str) {
    if let Some(k) = crate::types::SepKey::from_directive_key(key) {
        k.set(sep, value.to_string());
    }
}

pub(crate) fn parse_section_header(line: &str, line_num: usize) -> Result<SchemaSection> {
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

    // 后端标记：只认 `xlsx`（默认 tbl）。`index=` 是历史选项，继续忽略；
    // 其它未知 token 直接报错，避免把拼错的 `xls` 当注释吞掉。
    let mut storage = GroupStorage::Tbl;
    for opt in parts.iter().skip(1) {
        if *opt == "xlsx" {
            storage = GroupStorage::Xlsx;
        } else if opt.starts_with("index=") {
            // index 永远是 id（@01.10），保留兼容旧模板
        } else if *opt == "tbl" {
            storage = GroupStorage::Tbl;
        } else {
            bail!(
                "line {}: 未知 section 选项 {:?}（只支持 xlsx / tbl）",
                line_num + 1, opt
            );
        }
    }
    Ok(SchemaSection {
        group: group.trim().to_string(),
        name: name.trim().to_string(),
        mode,
        storage,
        fields: Vec::new(),
        preset: Vec::new(),
        display: None,

        rules: Vec::new(),
    })
}

pub(crate) fn validate_storage_consistency(sections: &[SchemaSection]) -> Result<()> {
    let mut seen: std::collections::HashMap<&str, GroupStorage> = std::collections::HashMap::new();
    for sec in sections {
        match seen.get(sec.group.as_str()) {
            Some(&s) if s != sec.storage => {
                bail!(
                    "[{}/{}] 与同组其它 section 的 storage 不一致（同组必须统一为 tbl 或 xlsx）",
                    sec.group, sec.name
                );
            }
            Some(_) => {}
            None => {
                seen.insert(sec.group.as_str(), sec.storage);
            }
        }
    }
    Ok(())
}

pub(crate) fn write_section_header(s: &mut String, sec: &SchemaSection) {
    use std::fmt::Write;
    let mode = match sec.mode {
        SchemaMode::Table => "table",
        SchemaMode::Constant => "constant",
        SchemaMode::Enum => "enum",
    };
    match sec.storage {
        GroupStorage::Tbl => { writeln!(s, "[{}/{}] {}", sec.group, sec.name, mode).unwrap(); }
        GroupStorage::Xlsx => { writeln!(s, "[{}/{}] {} xlsx", sec.group, sec.name, mode).unwrap(); }
    }
}

pub(crate) fn write_sep_diff(s: &mut String, sep: &SeparatorsSection) {
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

/// 写 `# @meta` 头。模板 `include_has_preset = true`；manifest 不写 has_preset。
pub(crate) fn write_meta(s: &mut String, meta: &SchemaMetadata, has_preset: bool) {
    use std::fmt::Write;
    if !meta.id.is_empty() { writeln!(s, "# @meta id: {}", meta.id).unwrap(); }
    if !meta.name.is_empty() && meta.name != meta.id {
        writeln!(s, "# @meta name: {}", meta.name).unwrap();
    }
    if !meta.category.is_empty() { writeln!(s, "# @meta category: {}", meta.category).unwrap(); }
    if !meta.version.is_empty() { writeln!(s, "# @meta version: {}", meta.version).unwrap(); }
    if !meta.created_at.is_empty() { writeln!(s, "# @meta created_at: {}", meta.created_at).unwrap(); }
    if !meta.source_template.is_empty() {
        writeln!(s, "# @meta source_template: {}", meta.source_template).unwrap();
    }
    if !meta.source_template_version.is_empty() {
        writeln!(s, "# @meta source_template_version: {}", meta.source_template_version).unwrap();
    }
    if has_preset {
        writeln!(s, "# @meta has_preset: true").unwrap();
    }
}

pub(crate) fn write_display(s: &mut String, sec: &SchemaSection) {
    use std::fmt::Write;
    if let Some(d) = &sec.display { writeln!(s, "# @display: {}", d).unwrap(); }
}

pub(crate) fn write_rules(s: &mut String, sec: &SchemaSection) {
    use std::fmt::Write;
    for rule in &sec.rules {
        let level = match rule.level {
            Severity::Error => "",
            Severity::Warning => " [warn]",
            Severity::Info => " [info]",
        };
        writeln!(s, "# @rule: {} = {}{}", rule.column, rule.expr_string(), level).unwrap();
    }
}
