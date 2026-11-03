//! schema 文档的共享语法工具：meta/sep、section 头、rule/display 词法、写盘 helper。
//!
//! 这里**不做格式策略**：某个格式允不允许字段行 / preset / 某个版本，由对应格式模块决定。

use anyhow::{Result, bail};

use crate::rules::{
    function_spec, CompareOp, Condition, FunctionStatus, ParamKind, Rule, RuleExpr, RuleKind,
    RuleScope, RuleSource, RuleTarget, Severity, ValueExpr,
};
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

/// 表达式节点：函数调用，或一个参数（裸 token / 引号字符串）。
#[derive(Debug, Clone, PartialEq)]
enum ExprArg {
    Call { name: String, args: Vec<ExprArg> },
    Bare(String),
    Quoted(String),
}

fn parse_expr_arg(src: &str) -> Result<ExprArg, String> {
    let s = src.trim();
    if s.is_empty() { return Ok(ExprArg::Bare(String::new())); }
    if s.starts_with('"') {
        let (val, rest) = parse_quoted(s)?;
        if !rest.trim().is_empty() {
            return Err(format!("引号字符串后有多余内容: \"{}\"", rest.trim()));
        }
        return Ok(ExprArg::Quoted(val));
    }
    if let Some(open) = s.find('(') {
        let name = s[..open].trim();
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            let (args, rest) = parse_call_args(s, open)?;
            if !rest.trim().is_empty() {
                return Err(format!("函数调用后有多余内容: \"{}\"", rest.trim()));
            }
            return Ok(ExprArg::Call { name: name.to_string(), args });
        }
    }
    Ok(ExprArg::Bare(s.to_string()))
}

fn parse_quoted(s: &str) -> Result<(String, &str), String> {
    let mut out = String::new();
    let mut esc = false;
    for (i, c) in s[1..].char_indices() {
        if esc {
            match c {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                _ => { out.push('\\'); out.push(c); }
            }
            esc = false;
        } else if c == '\\' {
            esc = true;
        } else if c == '"' {
            return Ok((out, &s[1 + i + c.len_utf8()..]));
        } else {
            out.push(c);
        }
    }
    Err("参数里的引号没有闭合".to_string())
}

fn parse_call_args(s: &str, open: usize) -> Result<(Vec<ExprArg>, &str), String> {
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    let mut end = None;
    for (i, c) in s[open..].char_indices() {
        if in_str {
            if esc { esc = false; }
            else if c == '\\' { esc = true; }
            else if c == '"' { in_str = false; }
        } else if c == '"' {
            in_str = true;
        } else if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth = depth.saturating_sub(1);
            if depth == 0 { end = Some(open + i); break; }
        }
    }
    if in_str { return Err("参数里的引号没有闭合".to_string()); }
    let end = end.ok_or_else(|| "函数调用的括号没有闭合".to_string())?;
    let inner = &s[open + 1..end];
    let mut args = Vec::new();
    if !inner.trim().is_empty() {
        for piece in split_top_level(inner, ',')? {
            args.push(parse_expr_arg(piece)?);
        }
    }
    Ok((args, &s[end + 1..]))
}

fn split_top_level(s: &str, delim: char) -> Result<Vec<&str>, String> {
    let mut out = Vec::new();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut esc = false;
    let mut start = 0;
    for (i, c) in s.char_indices() {
        if in_str {
            if esc { esc = false; }
            else if c == '\\' { esc = true; }
            else if c == '"' { in_str = false; }
        } else if c == '"' {
            in_str = true;
        } else if c == '(' {
            depth += 1;
        } else if c == ')' {
            depth -= 1;
            if depth < 0 { return Err("括号不匹配".to_string()); }
        } else if c == delim && depth == 0 {
            out.push(&s[start..i]);
            start = i + c.len_utf8();
        }
    }
    if in_str { return Err("参数里的引号没有闭合".to_string()); }
    if depth != 0 { return Err("括号不匹配".to_string()); }
    out.push(&s[start..]);
    Ok(out)
}

fn is_numeric_token(s: &str) -> bool {
    let chars_ok = s.chars().all(|c| {
        c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'e' | 'E')
    });
    chars_ok && s.parse::<f64>().is_ok()
}

pub(crate) fn parse_rule_body(body: &str) -> Result<Rule, String> {
    let (head, level) = split_rule_level(body)?;
    let (target, expr_text) = parse_rule_target(head)?;
    if expr_text.trim().is_empty() {
        return Err("规则缺少表达式".to_string());
    }
    let arg = parse_expr_arg(expr_text)?;
    let expr = parse_rule_expr(&arg, &target)?;
    Ok(Rule { target, expr, level, source: RuleSource::User })
}

/// 剥掉行尾的 `[error]` / `[warn]` / `[info]`。
fn split_rule_level(body: &str) -> Result<(&str, Severity), String> {
    let mut expr = body.trim();
    let mut level = Severity::Error;
    if let Some(stripped) = expr.strip_suffix(']') {
        if let Some(open) = stripped.rfind('[') {
            let raw = stripped[open + 1..].trim();
            match Severity::parse(raw) {
                Some(lv) => {
                    level = lv;
                    expr = stripped[..open].trim();
                }
                None => return Err(format!(
                    "未知的规则级别 \"{}\"（只接受 error / warn / info）", raw
                )),
            }
        }
    }
    Ok((expr, level))
}

/// `column <列名>: ...` / `table: ...`
fn parse_rule_target(head: &str) -> Result<(RuleTarget, &str), String> {
    let head = head.trim();
    if let Some(rest) = head.strip_prefix("column") {
        if rest.is_empty() || !rest.starts_with(|c: char| c.is_whitespace()) {
            return Err("规则头应为 `column <列名>: <表达式>` 或 `table: <表达式>`".to_string());
        }
        let rest = rest.trim_start();
        let (col, expr) = rest.split_once(':')
            .ok_or_else(|| "`column` 后缺少 `:`：应为 `column <列名>: <表达式>`".to_string())?;
        let col = col.trim();
        if col.is_empty() || col.chars().any(|c| c.is_whitespace()) {
            return Err("`column` 后需要合法的列名".to_string());
        }
        return Ok((RuleTarget::Column(col.to_string()), expr.trim()));
    }
    if let Some(rest) = head.strip_prefix("table") {
        let rest = rest.trim_start();
        let expr = rest.strip_prefix(':')
            .ok_or_else(|| "`table` 后缺少 `:`：应为 `table: <表达式>`".to_string())?;
        return Ok((RuleTarget::Table, expr.trim()));
    }
    if head.starts_with(':') {
        return Err(
            "旧格式 `# @rule: <列> = ...` 已废弃；请改用 `# @rule column <列>: ...` \
             或 `# @rule table: ...`".to_string()
        );
    }
    Err("规则头应为 `column <列名>: <表达式>` 或 `table: <表达式>`".to_string())
}

fn parse_rule_expr(arg: &ExprArg, target: &RuleTarget) -> Result<RuleExpr, String> {
    let (name, args) = match arg {
        ExprArg::Call { name, args } => (name.as_str(), args.as_slice()),
        _ => return Err("规则表达式必须是函数调用".to_string()),
    };
    match name {
        "when" => {
            if args.len() != 2 {
                return Err(format!("when 需要 2 个参数（条件, 验证器），实际给了 {}", args.len()));
            }
            let condition = parse_condition(&args[0])?;
            let inner = parse_rule_expr(&args[1], target)?;
            Ok(RuleExpr::When { condition, inner: Box::new(inner) })
        }
        "and" => {
            if args.is_empty() {
                return Err("and 至少需要 1 个规则表达式".to_string());
            }
            let cs = args.iter()
                .map(|a| parse_rule_expr(a, target))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(RuleExpr::And(cs))
        }
        "or" => {
            if args.len() < 2 {
                return Err(format!("or 至少需要 2 个规则表达式，实际给了 {}", args.len()));
            }
            let cs = args.iter()
                .map(|a| parse_rule_expr(a, target))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(RuleExpr::Or(cs))
        }
        "not" => Err("规则级 not 不支持；not 只用于 when 的条件".to_string()),
        "required_if" => parse_required_if(args, target),
        _ => parse_validator(name, args, target).map(RuleExpr::Validator),
    }
}

/// `required_if` 是语法糖：展开为 `when(eq(条件列, 值), non_empty(col))`。
fn parse_required_if(args: &[ExprArg], target: &RuleTarget) -> Result<RuleExpr, String> {
    let spec = function_spec("required_if").expect("catalog");
    if !spec.scopes.contains(&target.scope()) {
        return Err("required_if() 不能用在当前作用域".to_string());
    }
    let (col, cond_col, value) = match target {
        RuleTarget::Column(anchor) => {
            if args.len() != 2 {
                return Err(format!(
                    "required_if 在列级规则里需要 2 个参数（条件列, 值），实际给了 {}", args.len()
                ));
            }
            (
                anchor.clone(),
                column_arg(&args[0], "required_if 的条件列")?,
                literal_arg(&args[1], "required_if 的值")?,
            )
        }
        RuleTarget::Table => {
            if args.len() != 3 {
                return Err(format!(
                    "required_if 在表级规则里需要 3 个参数（列, 条件列, 值），实际给了 {}", args.len()
                ));
            }
            (
                column_arg(&args[0], "required_if 的列")?,
                column_arg(&args[1], "required_if 的条件列")?,
                literal_arg(&args[2], "required_if 的值")?,
            )
        }
    };
    Ok(RuleExpr::When {
        condition: Condition::Cmp {
            left: ValueExpr::Column(cond_col),
            op: CompareOp::Eq,
            right: ValueExpr::Literal(value),
        },
        inner: Box::new(RuleExpr::Validator(RuleKind::NonEmpty { column: col })),
    })
}

fn parse_condition(arg: &ExprArg) -> Result<Condition, String> {
    let (name, args) = match arg {
        ExprArg::Call { name, args } => (name.as_str(), args.as_slice()),
        _ => return Err("when 的第一个参数必须是条件函数，例如 eq(type, 1)".to_string()),
    };
    match name {
        "eq" | "ne" | "lt" | "le" | "gt" | "ge" => {
            if args.len() != 2 {
                return Err(format!("{} 需要 2 个参数（列/值, 列/值），实际给了 {}", name, args.len()));
            }
            let op = match name {
                "eq" => CompareOp::Eq,
                "ne" => CompareOp::Ne,
                "lt" => CompareOp::Lt,
                "le" => CompareOp::Le,
                "gt" => CompareOp::Gt,
                _ => CompareOp::Ge,
            };
            Ok(Condition::Cmp {
                left: parse_value_expr(&args[0])?,
                op,
                right: parse_value_expr(&args[1])?,
            })
        }
        "in" => {
            if args.len() < 2 {
                return Err("in 至少需要 2 个参数（列/值, 候选值...）".to_string());
            }
            let value = parse_value_expr(&args[0])?;
            let mut values = Vec::new();
            for a in &args[1..] {
                values.push(parse_value_expr(a)?);
            }
            Ok(Condition::In { value, values })
        }
        "match" => {
            if args.len() != 2 {
                return Err(format!("match 需要 2 个参数（列/值, 正则），实际给了 {}", args.len()));
            }
            Ok(Condition::Match {
                value: parse_value_expr(&args[0])?,
                pattern: literal_arg(&args[1], "match 的第二个参数")?,
            })
        }
        "and" | "or" => {
            if args.len() < 2 {
                return Err(format!("{} 至少需要 2 个条件，实际给了 {}", name, args.len()));
            }
            let cs = args.iter()
                .map(parse_condition)
                .collect::<Result<Vec<_>, _>>()?;
            Ok(if name == "and" { Condition::And(cs) } else { Condition::Or(cs) })
        }
        "not" => {
            if args.len() != 1 {
                return Err(format!("not 需要 1 个条件，实际给了 {}", args.len()));
            }
            Ok(Condition::Not(Box::new(parse_condition(&args[0])?)))
        }
        _ => Err(format!(
            "未知条件函数 \"{}\"（支持 eq / ne / lt / le / gt / ge / in / match / and / or / not）",
            name
        )),
    }
}

fn parse_value_expr(arg: &ExprArg) -> Result<ValueExpr, String> {
    match arg {
        ExprArg::Quoted(s) => Ok(ValueExpr::Literal(s.clone())),
        ExprArg::Bare(s) => {
            if s.is_empty() {
                return Err("条件的值不能为空".to_string());
            }
            if s == "true" || s == "false" || is_numeric_token(s) {
                Ok(ValueExpr::Literal(s.clone()))
            } else {
                Ok(ValueExpr::Column(s.clone()))
            }
        }
        ExprArg::Call { name, .. } => Err(format!(
            "值位置暂不支持函数调用 {}()（跨表 ref 尚未实现）", name
        )),
    }
}

fn literal_arg(arg: &ExprArg, what: &str) -> Result<String, String> {
    match arg {
        ExprArg::Bare(s) | ExprArg::Quoted(s) => Ok(s.clone()),
        ExprArg::Call { name, .. } =>
            Err(format!("{}不支持函数调用 {}()", what, name)),
    }
}

fn column_arg(arg: &ExprArg, what: &str) -> Result<String, String> {
    let s = literal_arg(arg, what)?;
    if s.trim().is_empty() {
        return Err(format!("{}需要列名，不能为空", what));
    }
    Ok(s)
}

fn check_arity(n: usize, fixed: usize, variadic: bool, name: &str) -> Result<(), String> {
    if variadic {
        if n < fixed + 1 {
            return Err(format!(
                "{} 至少需要 {} 个参数，实际给了 {}", name, fixed + 1, n
            ));
        }
    } else if n != fixed {
        return Err(format!("{} 需要 {} 个参数，实际给了 {}", name, fixed, n));
    }
    Ok(())
}

/// 根据函数目录解析一个验证器叶子。
fn parse_validator(name: &str, args: &[ExprArg], target: &RuleTarget) -> Result<RuleKind, String> {
    let spec = function_spec(name)
        .ok_or_else(|| format!("未知验证器/函数 \"{}\"（见 docs/16 函数目录）", name))?;
    if spec.status == FunctionStatus::Planned {
        return Err(format!("{}() 尚未实现", name));
    }
    if !spec.scopes.contains(&target.scope()) {
        let scope = match target.scope() {
            RuleScope::Column => "列级",
            RuleScope::Table => "表级",
        };
        return Err(format!("{}() 不能用在{}规则里", name, scope));
    }

    // 列级作用域：省略主列参数，由 column <列名> 补。
    let omit_primary = target.scope() == RuleScope::Column && spec.primary_column.is_some();
    let primary_idx = spec.primary_column;
    let fixed: Vec<ParamKind> = spec.params.iter().enumerate()
        .filter(|(i, _)| !(omit_primary && Some(*i) == primary_idx))
        .map(|(_, k)| *k)
        .collect();
    check_arity(args.len(), fixed.len(), spec.variadic.is_some(), name)?;

    // 把参数按签名转换成字符串（列名 / 字面量都先保持文本）。
    let mut kinds = fixed.clone();
    if let Some(v) = spec.variadic {
        for _ in fixed.len()..args.len() { kinds.push(v); }
    }
    let mut vals = Vec::new();
    for (arg, kind) in args.iter().zip(kinds.iter()) {
        vals.push(match kind {
            ParamKind::Column => column_arg(arg, &format!("{} 的列参数", name))?,
            ParamKind::Text | ParamKind::Regex | ParamKind::Bound
            | ParamKind::CompareOp | ParamKind::NodeColumn =>
                literal_arg(arg, &format!("{} 的参数", name))?,
        });
    }

    // 补回省略的主列，得到完整签名顺序的参数。
    let full: Vec<String> = if let (true, Some(idx)) = (omit_primary, primary_idx) {
        let anchor = target.column()
            .ok_or_else(|| "列级规则缺少锚点列".to_string())?;
        let mut v = vals.clone();
        v.insert(idx, anchor.to_string());
        v
    } else {
        vals.clone()
    };

    let get = |i: usize| -> Result<String, String> {
        full.get(i).cloned().ok_or_else(|| format!("{} 缺少第 {} 个参数", name, i + 1))
    };
    Ok(match name {
        "unique" => RuleKind::Unique { column: get(0)? },
        "non_empty" => RuleKind::NonEmpty { column: get(0)? },
        "positive_int" => RuleKind::PositiveInt { column: get(0)? },
        "upper_snake" => RuleKind::UpperSnake { column: get(0)? },
        "range" | "len" => {
            let column = get(0)?;
            let min = get(1)?;
            let max = get(2)?;
            if name == "range" {
                RuleKind::Range { column, min, max }
            } else {
                RuleKind::Len { column, min, max }
            }
        }
        "one_of" => {
            let column = get(0)?;
            let values = full[1..].to_vec();
            RuleKind::OneOf { column, values }
        }
        "matches" => RuleKind::Matches { column: get(0)?, pattern: get(1)? },
        "ref_exists" => {
            let column = get(0)?;
            let target = get(1)?;
            let (node, col) = target.split_once('.')
                .ok_or_else(|| "ref_exists 需要 `<节点>.<列>`".to_string())?;
            let node = node.trim();
            let col = col.trim();
            if node.is_empty() || col.is_empty()
                || node.chars().any(|c| c.is_whitespace())
                || col.chars().any(|c| c.is_whitespace()) {
                return Err("ref_exists 需要 `<节点>.<列>`（两侧都不能为空或含空白）".to_string());
            }
            RuleKind::RefExists {
                column,
                target_node: node.to_string(),
                target_column: col.to_string(),
            }
        }
        "compare" => {
            let column = get(0)?;
            let raw_op = get(1)?;
            let op = CompareOp::parse(&raw_op)
                .ok_or_else(|| format!("未知的比较运算符 \"{}\"（支持 < <= > >= == !=）", raw_op))?;
            RuleKind::Compare { column, op, other: get(2)? }
        }
        "all_non_empty" => RuleKind::AllNonEmpty { columns: full.clone() },
        "at_least_one" => RuleKind::AtLeastOne { columns: full.clone() },
        "exactly_one" => RuleKind::ExactlyOne { columns: full.clone() },
        "row_count" => RuleKind::RowCount { min: get(0)?, max: get(1)? },
        "id_contiguous" => RuleKind::IdContiguous,
        "unique_combination" => RuleKind::UniqueCombination { columns: full.clone() },
        _ => return Err(format!("未知验证器 \"{}\"", name)),
    })
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
        writeln!(s, "# @rule {}{}", rule.body_string(), level).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(body: &str) -> Rule {
        parse_rule_body(body).expect("parse rule")
    }

    #[test]
    fn rule_round_trip() {
        let cases = [
            "column value: non_empty()",
            r#"column value: when(eq(type, 1), matches("^\d+#\d+$"))"#,
            r#"column value: when(or(eq(type, 2), eq(type, 3)), matches("^[A-Z_]+$"))"#,
            "column value: when(and(in(type, 1, 2), ge(quality, 2)), non_empty())",
            r#"column value: when(match(code, "^SKILL_"), non_empty())"#,
            "column value: when(not(eq(type, 0)), range(1, 99))",
            r#"column value: and(non_empty(), matches("^\d+$"))"#,
            r#"column value: or(matches("^\d+$"), matches("^N/A$"))"#,
            // 组和函数都用 and/or 连接：规则层混合组合、条件层与规则层各自嵌套。
            "column value: and(unique(), or(non_empty(), range(1, 9)))",
            r#"column value: when(or(eq(type, 1), and(ge(quality, 2), match(code, "^A"))), or(range(1, 9), non_empty()))"#,
            "table: all_non_empty(value, price)",
            "table: and(all_non_empty(value, price), at_least_one(a, b))",
            "table: row_count(1, 1000)",
            "table: id_contiguous()",
            "table: unique_combination(type, level)",
            "table: when(eq(type, 1), non_empty(value))",
        ];
        for body in cases {
            let rule = parse(body);
            let printed = rule.body_string();
            let back = parse(&printed);
            assert_eq!(back, rule, "round-trip failed: {} -> {}", body, printed);
        }
    }

    #[test]
    fn required_if_expands_to_when() {
        let r = parse("column value: required_if(type, 1)");
        assert_eq!(r.body_string(), "column value: when(eq(type, 1), non_empty())");
        let r = parse("table: required_if(value, type, 1)");
        assert_eq!(r.body_string(), "table: when(eq(type, 1), non_empty(value))");
    }

    #[test]
    fn quoted_and_bare_values_are_distinct_kinds() {
        let r = parse(r#"column value: when(in(type, 1, "1", other), non_empty())"#);
        match r.expr {
            RuleExpr::When { condition: Condition::In { value, values }, .. } => {
                assert_eq!(value, ValueExpr::Column("type".into()));
                assert_eq!(values[0], ValueExpr::Literal("1".into()));
                assert_eq!(values[1], ValueExpr::Literal("1".into()));
                assert_eq!(values[2], ValueExpr::Column("other".into()));
            }
            other => panic!("{:?}", other),
        }
    }

    #[test]
    fn rule_parse_errors_are_explicit() {
        let e = parse_rule_body("column value: non_empty(value)").unwrap_err();
        assert!(e.contains("non_empty 需要 0 个参数"), "{e}");

        let e = parse_rule_body("table: non_empty()").unwrap_err();
        assert!(e.contains("non_empty 需要 1 个参数"), "{e}");

        let e = parse_rule_body("column value: all_non_empty(value, price)").unwrap_err();
        assert!(e.contains("不能用在列级"), "{e}");

        let e = parse_rule_body("column value: when(eq(type), non_empty())").unwrap_err();
        assert!(e.contains("eq 需要 2 个参数"), "{e}");

        let e = parse_rule_body("column value: when(type == 1, non_empty())").unwrap_err();
        assert!(e.contains("必须是条件函数"), "{e}");

        let e = parse_rule_body("column value: when(and(eq(type, 1)), non_empty())").unwrap_err();
        assert!(e.contains("至少需要 2 个条件"), "{e}");

        let e = parse_rule_body("comumn value: non_empty()").unwrap_err();
        assert!(e.contains("column") || e.contains("table"), "{e}");
    }

    #[test]
    fn level_suffix_parses() {
        let rule = parse(r#"column value: when(eq(type, 1), non_empty()) [warn]"#);
        assert_eq!(rule.level, Severity::Warning);
    }

    #[test]
    fn regex_backslash_is_preserved() {
        for src in [
            r#"column value: when(match(code, "^\d+$"), non_empty())"#,
            r#"column value: when(match(code, "^\d+$"), non_empty())"#,
        ] {
            let r = parse(src);
            match r.expr {
                RuleExpr::When { condition: Condition::Match { pattern, .. }, .. } =>
                    assert_eq!(pattern, r"^\d+$", "source = {}", src),
                other => panic!("{:?}", other),
            }
        }
    }

    #[test]
    fn rename_column_updates_condition_refs() {
        let mut r = parse(r#"column value: when(and(eq(type, 1), match(value, "^a")), non_empty())"#);
        r.rename_column("type", "kind");
        r.rename_column("value", "text");
        assert_eq!(
            r.body_string(),
            r#"column text: when(and(eq(kind, 1), match(text, "^a")), non_empty())"#
        );
    }
}
