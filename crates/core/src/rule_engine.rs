//! 规则表达式引擎（@16）：schema 校验 + 逐行/全表求值。
//!
//! 与 `validate.rs` 的分工：
//! - `validate.rs` 持有列规格 / `RuleIndex` / `ValidationError` / `ValueClass`；
//! - 本模块只处理 `Rule` / `RuleExpr` / `RuleKind`，产出 `ValidationError`。

use std::collections::{HashMap, HashSet};

use crate::rules::{
    CompareOp, Condition, Granularity, NodeMode, Rule, RuleExpr, RuleKind, RuleScope, RuleTarget,
    Severity, ValueExpr,
};
use crate::validate::{
    ColumnSpec, RuleIndex, ValidationCode, ValidationError, ValueClass, TABLE_ROW,
};

fn col_idx(columns: &[ColumnSpec], column: &str) -> Option<usize> {
    columns.iter().position(|(c, _)| c == column)
}

fn is_numeric(c: ValueClass) -> bool {
    matches!(c, ValueClass::Int | ValueClass::Float)
}

fn class_of(columns: &[ColumnSpec], column: &str) -> Option<ValueClass> {
    col_idx(columns, column).and_then(|i| columns[i].1)
}

fn parse_bound(raw: &str) -> Result<Option<f64>, String> {
    let t = raw.trim();
    if t.is_empty() { return Ok(None); }
    t.parse::<f64>().map(Some).map_err(|_| format!("\"{}\" 不是数字", t))
}

fn check_bounds(min: &str, max: &str) -> Result<(), String> {
    let lo = parse_bound(min)?;
    let hi = parse_bound(max)?;
    if lo.is_none() && hi.is_none() {
        return Err("至少填一侧端点（两端都空等于没有规则）".to_string());
    }
    if let (Some(a), Some(b)) = (lo, hi) {
        if a > b { return Err(format!("下限 {} 大于上限 {}", a, b)); }
    }
    Ok(())
}

// ────────────────────────────────────────────────────────────────────────────
// Schema 校验
// ────────────────────────────────────────────────────────────────────────────

fn condition_column_class(
    columns: &[ColumnSpec],
    name: &str,
) -> Result<ValueClass, (ValidationCode, String)> {
    match col_idx(columns, name) {
        None => Err((ValidationCode::RuleColumnMissing,
            format!("条件引用的列 \"{}\" 不存在", name))),
        Some(i) => match columns[i].1 {
            Some(c) => Ok(c),
            None => Err((ValidationCode::RuleTypeIncompatible,
                format!("条件引用的列 \"{}\" 是复合类型，不支持校验规则", name))),
        },
    }
}

fn is_column(v: &ValueExpr) -> bool { matches!(v, ValueExpr::Column(_)) }

fn check_value_ref(v: &ValueExpr, columns: &[ColumnSpec]) -> Option<(ValidationCode, String)> {
    match v {
        ValueExpr::Literal(_) => None,
        ValueExpr::Column(name) => condition_column_class(columns, name).err(),
    }
}

fn check_order_operand(v: &ValueExpr, columns: &[ColumnSpec]) -> Option<(ValidationCode, String)> {
    match v {
        ValueExpr::Literal(s) => {
            if s.trim().parse::<f64>().is_err() {
                Some((ValidationCode::RuleParamInvalid,
                    format!("条件里的字面量 \"{}\" 不是数字，不能做大小比较", s)))
            } else { None }
        }
        ValueExpr::Column(name) => match condition_column_class(columns, name) {
            Err(e) => Some(e),
            Ok(c) if !is_numeric(c) => Some((ValidationCode::RuleTypeIncompatible,
                format!("条件列 \"{}\" 不是数值类型，不能做大小比较", name))),
            Ok(_) => None,
        },
    }
}

/// 行级条件的 schema 校验。
pub fn condition_schema_error(
    cond: &Condition,
    columns: &[ColumnSpec],
) -> Option<(ValidationCode, String)> {
    match cond {
        Condition::And(cs) | Condition::Or(cs) =>
            cs.iter().find_map(|c| condition_schema_error(c, columns)),
        Condition::Not(c) => condition_schema_error(c, columns),
        Condition::Cmp { left, op, right } => {
            if let Some(e) = check_value_ref(left, columns) { return Some(e); }
            if let Some(e) = check_value_ref(right, columns) { return Some(e); }
            if matches!(op, CompareOp::Lt | CompareOp::Le | CompareOp::Gt | CompareOp::Ge) {
                if let Some(e) = check_order_operand(left, columns) { return Some(e); }
                if let Some(e) = check_order_operand(right, columns) { return Some(e); }
            }
            if !is_column(left) && !is_column(right) {
                return Some((ValidationCode::RuleParamInvalid,
                    "条件至少引用一列（两边都是字面量的条件恒真/恒假）".to_string()));
            }
            None
        }
        Condition::In { value, values } => {
            if let Some(e) = check_value_ref(value, columns) { return Some(e); }
            for v in values {
                if let Some(e) = check_value_ref(v, columns) { return Some(e); }
            }
            if !is_column(value) {
                return Some((ValidationCode::RuleParamInvalid,
                    "in 的第一个参数必须是列".to_string()));
            }
            if values.is_empty() {
                return Some((ValidationCode::RuleParamInvalid,
                    "in 至少需要一个候选值".to_string()));
            }
            None
        }
        Condition::Match { value, pattern } => {
            if let Some(e) = check_value_ref(value, columns) { return Some(e); }
            if !is_column(value) {
                return Some((ValidationCode::RuleParamInvalid,
                    "match 的第一个参数必须是列".to_string()));
            }
            if pattern.trim().is_empty() {
                return Some((ValidationCode::RuleParamInvalid,
                    "正则不能为空（空正则匹配一切，等于没写）".to_string()));
            }
            if let Err(e) = regex::Regex::new(pattern) {
                return Some((ValidationCode::RuleParamInvalid, format!("正则不合法: {}", e)));
            }
            None
        }
    }
}

/// 给 GUI 用：草稿条件的 schema 错误。
pub fn condition_ui_error(condition: &Condition, columns: &[ColumnSpec]) -> Option<String> {
    condition_schema_error(condition, columns).map(|(_, msg)| msg)
}

fn validator_schema_error(
    v: &RuleKind,
    scope: RuleScope,
    anchor: Option<&str>,
    columns: &[ColumnSpec],
    cidx: &RuleIndex,
    node_name: &str,
) -> Option<(ValidationCode, String)> {
    if !v.scope_ok(scope) {
        return Some((ValidationCode::RuleTypeIncompatible,
            format!("{}() 不能用在{}规则里", v.name(),
                if scope == RuleScope::Column { "列级" } else { "表级" })));
    }
    if scope == RuleScope::Column {
        let Some(anchor) = anchor else {
            return Some((ValidationCode::RuleParamInvalid, "列级规则缺少锚点列".to_string()));
        };
        match v.primary_column() {
            Some(col) if col == anchor => {}
            _ => return Some((ValidationCode::RuleParamInvalid,
                format!("列级规则只能使用锚点列 \"{}\" 作为主列", anchor))),
        }
    }

    // 所有引用的列存在且是标量；列清单不能重复。
    let cols = v.columns();
    let mut seen = HashSet::new();
    for c in &cols {
        let Some(i) = col_idx(columns, c) else {
            return Some((ValidationCode::RuleColumnMissing,
                format!("规则引用的列 \"{}\" 不存在", c)));
        };
        if columns[i].1.is_none() {
            return Some((ValidationCode::RuleTypeIncompatible,
                format!("列 \"{}\" 的类型不支持校验规则（复合类型不参与）", c)));
        }
        if !seen.insert(*c) {
            return Some((ValidationCode::RuleParamInvalid,
                format!("规则里的列 \"{}\" 重复", c)));
        }
    }

    let primary = v.primary_column().and_then(|c| class_of(columns, c));
    match v {
        RuleKind::Range { min, max, .. } | RuleKind::Len { min, max, .. } => {
            if let Err(msg) = check_bounds(min, max) {
                return Some((ValidationCode::RuleParamInvalid, msg));
            }
            let text = matches!(v, RuleKind::Len { .. });
            match primary {
                Some(c) if text && c == ValueClass::Text => {}
                Some(c) if !text && is_numeric(c) => {}
                _ => return Some((ValidationCode::RuleTypeIncompatible,
                    format!("{}() 不适用于该列的类型", v.name()))),
            }
        }
        RuleKind::OneOf { values, .. } => {
            if values.is_empty() {
                return Some((ValidationCode::RuleParamInvalid,
                    "one_of 至少需要一个候选值".to_string()));
            }
        }
        RuleKind::Matches { pattern, .. } => {
            if primary != Some(ValueClass::Text) {
                return Some((ValidationCode::RuleTypeIncompatible,
                    "matches() 只适用于文本列".to_string()));
            }
            if pattern.trim().is_empty() {
                return Some((ValidationCode::RuleParamInvalid,
                    "正则不能为空（空正则匹配一切，等于没写）".to_string()));
            }
            if let Err(e) = regex::Regex::new(pattern) {
                return Some((ValidationCode::RuleParamInvalid, format!("正则不合法: {}", e)));
            }
        }
        RuleKind::RefExists { target_node, target_column, .. } => {
            if target_node == node_name && Some(target_column.as_str()) == v.primary_column() {
                return Some((ValidationCode::RuleParamInvalid,
                    "ref_exists 的目标不能是本列自己（恒成立，没有意义）".to_string()));
            }
            if !cidx.node_exists(target_node) {
                return Some((ValidationCode::RuleTargetMissing,
                    format!("ref_exists 的目标节点 \"{}\" 不存在", target_node)));
            }
            if cidx.is_ambiguous(target_node) {
                return Some((ValidationCode::RuleTargetAmbiguous,
                    format!("ref_exists 的目标节点 \"{}\" 在项目里重名", target_node)));
            }
            if !cidx.column_exists(target_node, target_column) {
                return Some((ValidationCode::RuleTargetColumnMissing,
                    format!("ref_exists 的目标列 \"{}.{}\" 不存在", target_node, target_column)));
            }
            if cidx.column_class(target_node, target_column).is_none() {
                return Some((ValidationCode::RuleTypeIncompatible,
                    format!("ref_exists 的目标列 \"{}.{}\" 是复合类型，不支持引用校验",
                        target_node, target_column)));
            }
        }
        RuleKind::Compare { column, other, .. } => {
            if column == other {
                return Some((ValidationCode::RuleParamInvalid,
                    "compare 比较的两个列不能相同".to_string()));
            }
            match (class_of(columns, column), class_of(columns, other)) {
                (Some(a), Some(b)) if is_numeric(a) && is_numeric(b) => {}
                _ => return Some((ValidationCode::RuleTypeIncompatible,
                    "compare 的两列都必须是数值类型".to_string())),
            }
        }
        RuleKind::PositiveInt { .. } => {
            if !matches!(primary, Some(ValueClass::Int)) {
                return Some((ValidationCode::RuleTypeIncompatible,
                    "positive_int() 只适用于整数列".to_string()));
            }
        }
        RuleKind::UpperSnake { .. } => {
            if primary != Some(ValueClass::Text) {
                return Some((ValidationCode::RuleTypeIncompatible,
                    "upper_snake() 只适用于文本列".to_string()));
            }
        }
        RuleKind::AllNonEmpty { columns } | RuleKind::AtLeastOne { columns }
        | RuleKind::ExactlyOne { columns } | RuleKind::UniqueCombination { columns } => {
            if columns.len() < 2 {
                return Some((ValidationCode::RuleParamInvalid,
                    format!("{} 至少需要 2 个列", v.name())));
            }
        }
        RuleKind::RowCount { min, max } => {
            if let Err(msg) = check_bounds(min, max) {
                return Some((ValidationCode::RuleParamInvalid,
                    format!("row_count: {}", msg)));
            }
        }
        RuleKind::IdContiguous => {}
        RuleKind::NoEmptyRows => {
            return Some((ValidationCode::RuleParamInvalid,
                "no_empty_rows() 尚未实现".to_string()));
        }
        _ => {}
    }
    None
}

fn expr_schema_error(
    expr: &RuleExpr,
    scope: RuleScope,
    anchor: Option<&str>,
    columns: &[ColumnSpec],
    cidx: &RuleIndex,
    node_name: &str,
) -> Option<(ValidationCode, String)> {
    match expr {
        RuleExpr::Validator(v) =>
            validator_schema_error(v, scope, anchor, columns, cidx, node_name),
        RuleExpr::When { condition, inner } => {
            if let Some(e) = condition_schema_error(condition, columns) { return Some(e); }
            expr_schema_error(inner, scope, anchor, columns, cidx, node_name)
        }
        RuleExpr::And(cs) => {
            if cs.is_empty() {
                return Some((ValidationCode::RuleParamInvalid,
                    "and 至少需要 1 个子表达式".to_string()));
            }
            cs.iter().find_map(|c| expr_schema_error(c, scope, anchor, columns, cidx, node_name))
        }
        RuleExpr::Or(cs) => {
            if cs.len() < 2 {
                return Some((ValidationCode::RuleParamInvalid,
                    "or 至少需要 2 个子表达式".to_string()));
            }
            for c in cs {
                if c.has_table_granularity() {
                    return Some((ValidationCode::RuleParamInvalid,
                        "or 的子表达式必须是逐行规则（v1 不支持组合全表函数）".to_string()));
                }
            }
            cs.iter().find_map(|c| expr_schema_error(c, scope, anchor, columns, cidx, node_name))
        }
    }
}

fn rule_schema_error(
    rule: &Rule,
    mode: NodeMode,
    node_name: &str,
    columns: &[ColumnSpec],
    cidx: &RuleIndex,
) -> Option<(ValidationCode, String)> {
    match &rule.target {
        RuleTarget::Column(col) => {
            let Some(i) = col_idx(columns, col) else {
                return Some((ValidationCode::RuleColumnMissing,
                    format!("规则引用的列 \"{}\" 不存在", col)));
            };
            if columns[i].1.is_none() {
                return Some((ValidationCode::RuleTypeIncompatible,
                    format!("列 \"{}\" 的类型不支持校验规则（复合类型不参与）", col)));
            }
        }
        RuleTarget::Table => {
            if mode != NodeMode::Table {
                return Some((ValidationCode::RuleTypeIncompatible,
                    "table: 规则只能用于 Table 节点".to_string()));
            }
        }
    }
    expr_schema_error(
        &rule.expr, rule.target.scope(), rule.target.column(), columns, cidx, node_name,
    )
}

/// 节点级规则的 schema 校验（UI 规则面板 + 保存前）。
pub fn validate_rules_schema(
    mode: NodeMode,
    node_name: &str,
    columns: &[ColumnSpec],
    rules: &[Rule],
    cidx: &RuleIndex,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for (rule_idx, rule) in rules.iter().enumerate() {
        let level = rule.level;
        let col = rule.target_column().and_then(|c| col_idx(columns, c)).unwrap_or(0);
        if rule.source == crate::rules::RuleSource::User
            && rule.conflicts_with_builtin(mode)
        {
            errors.push(
                ValidationError::schema(
                    ValidationCode::RuleRedundant,
                    format!("该规则已由系统保证，{} 不会生效", rule.expr_string()),
                    col, None,
                ).with_field(rule.target_column().unwrap_or("").to_string())
                 .with_level(Severity::Warning)
                 .with_rule_index(rule_idx)
            );
            continue;
        }
        if let Some((code, msg)) = rule_schema_error(rule, mode, node_name, columns, cidx) {
            errors.push(
                ValidationError::schema(code, msg, col, None)
                    .with_field(rule.target_column().unwrap_or("").to_string())
                    .with_level(level)
                    .with_rule_index(rule_idx)
            );
        }
    }
    errors
}

/// 给 GUI 用：这条规则在当前列规格下是否可保存（只做形状/本地列检查）。
pub fn rule_ui_error(rule: &Rule, mode: NodeMode, node_name: &str, columns: &[ColumnSpec]) -> Option<String> {
    let cidx = RuleIndex::build_schema(&[]);
    rule_schema_error(rule, mode, node_name, columns, &cidx).map(|(_, msg)| msg)
}

/// 规则参数形状是否完整（UI 用；不检查列是否存在）。
pub fn rule_params_valid(expr: &RuleExpr) -> bool {
    match expr {
        RuleExpr::Validator(v) => validator_params_valid(v),
        RuleExpr::When { condition, inner } =>
            condition_params_valid(condition) && rule_params_valid(inner),
        RuleExpr::And(cs) => !cs.is_empty() && cs.iter().all(rule_params_valid),
        RuleExpr::Or(cs) => cs.len() >= 2
            && cs.iter().all(|c| !c.has_table_granularity() && rule_params_valid(c)),
    }
}

fn validator_params_valid(v: &RuleKind) -> bool {
    match v {
        RuleKind::Range { min, max, .. } | RuleKind::Len { min, max, .. } =>
            check_bounds(min, max).is_ok(),
        RuleKind::RowCount { min, max } => check_bounds(min, max).is_ok(),
        RuleKind::OneOf { values, .. } => !values.is_empty(),
        RuleKind::Matches { pattern, .. } =>
            !pattern.trim().is_empty() && regex::Regex::new(pattern).is_ok(),
        RuleKind::RefExists { target_node, target_column, .. } =>
            !target_node.is_empty() && !target_column.is_empty(),
        RuleKind::Compare { column, other, .. } => !column.is_empty() && !other.is_empty(),
        RuleKind::AllNonEmpty { columns } | RuleKind::AtLeastOne { columns }
        | RuleKind::ExactlyOne { columns } | RuleKind::UniqueCombination { columns } =>
            columns.len() >= 2 && columns.iter().all(|c| !c.trim().is_empty()),
        RuleKind::Unique { column } | RuleKind::NonEmpty { column }
        | RuleKind::PositiveInt { column } | RuleKind::UpperSnake { column } =>
            !column.trim().is_empty(),
        RuleKind::IdContiguous => true,
        RuleKind::NoEmptyRows => false,
    }
}

pub fn condition_params_valid(cond: &Condition) -> bool {
    fn value_ok(v: &ValueExpr) -> bool {
        match v {
            ValueExpr::Column(name) => !name.trim().is_empty(),
            ValueExpr::Literal(_) => true,
        }
    }
    match cond {
        Condition::Cmp { left, right, .. } =>
            value_ok(left) && value_ok(right) && (is_column(left) || is_column(right)),
        Condition::In { value, values } =>
            is_column(value) && !values.is_empty() && values.iter().all(value_ok),
        Condition::Match { value, pattern } =>
            is_column(value) && !pattern.trim().is_empty()
                && regex::Regex::new(pattern).is_ok(),
        Condition::And(cs) | Condition::Or(cs) =>
            cs.len() >= 2 && cs.iter().all(condition_params_valid),
        Condition::Not(c) => condition_params_valid(c),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 求值
// ────────────────────────────────────────────────────────────────────────────

struct EvalCtx<'a, F, G> {
    columns: &'a [ColumnSpec],
    cidx: &'a RuleIndex,
    row_count: usize,
    cell: &'a F,
    row_is_empty: &'a G,
    level: Severity,
    rule_index: usize,
}

type Push<'a> = dyn FnMut(ValidationError) + 'a;

fn cell_value<F>(ctx: &EvalCtx<'_, F, impl Fn(usize) -> bool>, row: usize, col: &str) -> String
where
    F: Fn(usize, &str) -> Option<String>,
{
    (ctx.cell)(row, col).unwrap_or_default()
}

fn push_error(
    push: &mut Push<'_>,
    ctx: &impl CtxLike,
    code: ValidationCode,
    row: usize,
    anchor: &str,
    affected: Vec<String>,
    message: String,
    value: &str,
) {
    let col = col_idx(ctx.columns(), anchor).unwrap_or(0);
    let field = affected.join("、");
    push(
        ValidationError::cell(code, message)
            .with_pos(row, col)
            .with_field(field)
            .with_value(value)
            .with_level(ctx.level())
            .with_rule_index(ctx.rule_index())
            .with_affected(affected)
    );
}

trait CtxLike {
    fn columns(&self) -> &[ColumnSpec];
    fn level(&self) -> Severity;
    fn rule_index(&self) -> usize;
}

impl<F, G> CtxLike for EvalCtx<'_, F, G> {
    fn columns(&self) -> &[ColumnSpec] { self.columns }
    fn level(&self) -> Severity { self.level }
    fn rule_index(&self) -> usize { self.rule_index }
}

fn eval_expr<F, G>(
    ctx: &EvalCtx<'_, F, G>,
    expr: &RuleExpr,
    allowed: &[bool],
    push: &mut Push<'_>,
) where
    F: Fn(usize, &str) -> Option<String>,
    G: Fn(usize) -> bool,
{
    match expr {
        RuleExpr::Validator(v) => eval_validator(ctx, v, allowed, push),
        RuleExpr::When { condition, inner } => {
            let mut next = vec![false; ctx.row_count];
            for row in 0..ctx.row_count {
                if allowed.get(row).copied().unwrap_or(false)
                    && eval_condition(condition, row, ctx.columns, ctx.cell)
                {
                    next[row] = true;
                }
            }
            eval_expr(ctx, inner, &next, push);
        }
        RuleExpr::And(cs) => {
            for c in cs { eval_expr(ctx, c, allowed, push); }
        }
        RuleExpr::Or(cs) => {
            // schema 校验已保证子表达式都是 Row 粒度。
            for row in 0..ctx.row_count {
                if !allowed.get(row).copied().unwrap_or(false) { continue; }
                if cs.iter().any(|c| row_passes(ctx, c, row)) { continue; }
                let affected: Vec<String> = {
                    let mut out = Vec::new();
                    for c in cs {
                        for col in c.columns() {
                            if !out.iter().any(|x| x == col) { out.push(col.to_string()); }
                        }
                    }
                    out
                };
                let anchor = affected.first().cloned().unwrap_or_default();
                let list = cs.iter()
                    .map(|c| c.expr_string(false))
                    .collect::<Vec<_>>()
                    .join("；");
                push_error(push, ctx, ValidationCode::RuleRowViolation, row, &anchor,
                    affected, format!("不满足任一条件：{}", list), "");
            }
        }
    }
}

/// 单个行是否通过某个 Row 粒度表达式（只用于 or）。
fn row_passes<F, G>(ctx: &EvalCtx<'_, F, G>, expr: &RuleExpr, row: usize) -> bool
where
    F: Fn(usize, &str) -> Option<String>,
    G: Fn(usize) -> bool,
{
    match expr {
        RuleExpr::Validator(v) => row_validator_issue(ctx, v, row).is_none(),
        RuleExpr::When { condition, inner } => {
            if !eval_condition(condition, row, ctx.columns, ctx.cell) { true }
            else { row_passes(ctx, inner, row) }
        }
        RuleExpr::And(cs) => cs.iter().all(|c| row_passes(ctx, c, row)),
        RuleExpr::Or(cs) => cs.iter().any(|c| row_passes(ctx, c, row)),
    }
}

fn eval_validator<F, G>(
    ctx: &EvalCtx<'_, F, G>,
    v: &RuleKind,
    allowed: &[bool],
    push: &mut Push<'_>,
) where
    F: Fn(usize, &str) -> Option<String>,
    G: Fn(usize) -> bool,
{
    match v.granularity() {
        Granularity::Row => {
            for row in 0..ctx.row_count {
                if !allowed.get(row).copied().unwrap_or(false) { continue; }
                if let Some(msg) = row_validator_issue(ctx, v, row) {
                    let affected: Vec<String> = v.columns().iter().map(|c| c.to_string()).collect();
                    let anchor = affected.first().cloned().unwrap_or_default();
                    push_error(push, ctx, msg.code, row, &anchor, affected, msg.message, &msg.value);
                }
            }
        }
        Granularity::Table => eval_table_validator(ctx, v, allowed, push),
    }
}

struct Issue {
    code: ValidationCode,
    message: String,
    value: String,
}

fn row_validator_issue<F, G>(
    ctx: &EvalCtx<'_, F, G>,
    v: &RuleKind,
    row: usize,
) -> Option<Issue>
where
    F: Fn(usize, &str) -> Option<String>,
    G: Fn(usize) -> bool,
{
    let empty_row = (ctx.row_is_empty)(row);
    let get = |col: &str| cell_value(ctx, row, col);
    match v {
        RuleKind::NonEmpty { column } => {
            if empty_row || !get(column).trim().is_empty() { return None; }
            Some(Issue { code: ValidationCode::RuleNotEmptyViolation,
                message: "该列必填，不能为空".into(), value: String::new() })
        }
        RuleKind::Range { column, min, max } => {
            let raw = get(column);
            let t = raw.trim();
            if t.is_empty() { return None; }
            let Ok(n) = t.parse::<f64>() else {
                return Some(Issue { code: ValidationCode::RuleRangeViolation,
                    message: format!("值 \"{}\" 不是数字，无法做区间校验", t), value: t.into() });
            };
            let lo = parse_bound(min).ok().flatten();
            let hi = parse_bound(max).ok().flatten();
            let bad = lo.map_or(false, |b| n < b) || hi.map_or(false, |b| n > b);
            if !bad { return None; }
            let range = match (lo, hi) {
                (Some(a), Some(b)) => format!("[{}, {}]", a, b),
                (Some(a), None) => format!("≥ {}", a),
                (None, Some(b)) => format!("≤ {}", b),
                (None, None) => String::new(),
            };
            Some(Issue { code: ValidationCode::RuleRangeViolation,
                message: format!("值 {} 不在允许区间 {} 内", n, range), value: t.into() })
        }
        RuleKind::Len { column, min, max } => {
            let raw = get(column);
            if raw.is_empty() { return None; }
            let n = raw.chars().count() as f64;
            let lo = parse_bound(min).ok().flatten();
            let hi = parse_bound(max).ok().flatten();
            let bad = lo.map_or(false, |b| n < b) || hi.map_or(false, |b| n > b);
            if !bad { return None; }
            let range = match (lo, hi) {
                (Some(a), Some(b)) => format!("[{}, {}]", a, b),
                (Some(a), None) => format!("≥ {}", a),
                (None, Some(b)) => format!("≤ {}", b),
                (None, None) => String::new(),
            };
            Some(Issue { code: ValidationCode::RuleLenViolation,
                message: format!("长度 {} 不在允许区间 {} 内", n as i64, range), value: raw })
        }
        RuleKind::OneOf { column, values } => {
            let raw = get(column);
            let class = class_of(ctx.columns, column).unwrap_or(ValueClass::Text);
            let nv = class.normalize(&raw);
            if nv.is_empty() { return None; }
            let set: HashSet<String> = values.iter().map(|v| class.normalize(v)).collect();
            if set.contains(&nv) { return None; }
            Some(Issue { code: ValidationCode::RuleOneOfViolation,
                message: format!("值 \"{}\" 不在允许集合 {{{}}} 内", nv, values.join(", ")),
                value: nv })
        }
        RuleKind::Matches { column, pattern } => {
            let raw = get(column);
            if raw.is_empty() { return None; }
            let Ok(re) = regex::Regex::new(pattern) else { return None; };
            if re.is_match(&raw) { return None; }
            Some(Issue { code: ValidationCode::RulePatternViolation,
                message: format!("值 \"{}\" 不匹配正则 {}", raw, pattern), value: raw })
        }
        RuleKind::Compare { column, op, other } => {
            let left = get(column);
            let right = get(other);
            let (lt, rt) = (left.trim(), right.trim());
            if lt.is_empty() || rt.is_empty() { return None; }
            let (Some(a), Some(b)) = (lt.parse::<f64>().ok(), rt.parse::<f64>().ok()) else {
                return Some(Issue { code: ValidationCode::RuleCompareViolation,
                    message: format!("值 \"{}\" 或 \"{}\" 不是数字，无法比较", lt, rt),
                    value: lt.into() });
            };
            let ok = match op {
                CompareOp::Lt => a < b,
                CompareOp::Le => a <= b,
                CompareOp::Gt => a > b,
                CompareOp::Ge => a >= b,
                CompareOp::Eq => a == b,
                CompareOp::Ne => a != b,
            };
            if ok { return None; }
            Some(Issue { code: ValidationCode::RuleCompareViolation,
                message: format!("本列值 {} 应 {} {} 列的值 {}", a, op.as_str(), other, b),
                value: lt.into() })
        }
        RuleKind::PositiveInt { column } => {
            let raw = get(column);
            let t = raw.trim();
            if t.is_empty() { return None; }
            match t.parse::<i64>() {
                Ok(n) if n <= 0 => Some(Issue { code: ValidationCode::RulePositiveIntViolation,
                    message: format!("值 {} 必须是正整数（不能为 0 或负数）", n), value: t.into() }),
                Err(_) => Some(Issue { code: ValidationCode::RulePositiveIntViolation,
                    message: format!("值 \"{}\" 必须是整数", t), value: t.into() }),
                _ => None,
            }
        }
        RuleKind::UpperSnake { column } => {
            let raw = get(column);
            if raw.is_empty() { return None; }
            if super::validate::is_valid_enum_entry_name(&raw) { return None; }
            Some(Issue { code: ValidationCode::RuleUpperSnakeViolation,
                message: format!("值 \"{}\" 不是 UPPER_SNAKE_CASE（需大写字母开头，只含大写字母/数字/下划线）", raw),
                value: raw })
        }
        RuleKind::RefExists { column, target_node, target_column } => {
            let raw = get(column);
            let class = class_of(ctx.columns, column).unwrap_or(ValueClass::Text);
            let nv = class.normalize(&raw);
            if nv.is_empty() { return None; }
            let Some((tclass, set)) = ctx.cidx.values(target_node, target_column) else { return None; };
            let target = tclass.normalize(&raw);
            if set.contains(&target) { return None; }
            let _ = nv;
            Some(Issue { code: ValidationCode::RuleRefValueMissing,
                message: format!("引用值 \"{}\" 不存在于 {}.{}", raw.trim(), target_node, target_column),
                value: raw })
        }
        RuleKind::AllNonEmpty { columns } => {
            if empty_row { return None; }
            let missing: Vec<&str> = columns.iter()
                .filter(|c| get(c).trim().is_empty())
                .map(|c| c.as_str())
                .collect();
            if missing.is_empty() { return None; }
            Some(Issue { code: ValidationCode::RuleRowViolation,
                message: format!("这些列都必须有值：{}", missing.join("、")), value: String::new() })
        }
        RuleKind::AtLeastOne { columns } => {
            if empty_row { return None; }
            if columns.iter().any(|c| !get(c).trim().is_empty()) { return None; }
            Some(Issue { code: ValidationCode::RuleRowViolation,
                message: format!("这些列至少需要一列有值：{}", columns.join("、")), value: String::new() })
        }
        RuleKind::ExactlyOne { columns } => {
            if empty_row { return None; }
            let n = columns.iter().filter(|c| !get(c).trim().is_empty()).count();
            if n == 1 { return None; }
            Some(Issue { code: ValidationCode::RuleRowViolation,
                message: format!("这些列必须恰好一列有值（当前 {} 列）：{}", n, columns.join("、")),
                value: String::new() })
        }
        // Table 粒度不会走到这里。
        _ => None,
    }
}

fn eval_table_validator<F, G>(
    ctx: &EvalCtx<'_, F, G>,
    v: &RuleKind,
    allowed: &[bool],
    push: &mut Push<'_>,
) where
    F: Fn(usize, &str) -> Option<String>,
    G: Fn(usize) -> bool,
{
    match v {
        RuleKind::Unique { column } => {
            let class = class_of(ctx.columns, column).unwrap_or(ValueClass::Text);
            let mut seen: HashMap<String, usize> = HashMap::new();
            for row in 0..ctx.row_count {
                if !allowed.get(row).copied().unwrap_or(false) { continue; }
                let raw = cell_value(ctx, row, column);
                let nv = class.normalize(&raw);
                if nv.is_empty() { continue; }
                if let Some(&first) = seen.get(&nv) {
                    push_error(push, ctx, ValidationCode::RuleUniqueViolation, row,
                        column, vec![column.clone()],
                        format!("值 \"{}\" 与第 {} 行重复", nv, first + 1), &nv);
                } else {
                    seen.insert(nv, row);
                }
            }
        }
        RuleKind::UniqueCombination { columns } => {
            let mut seen: HashMap<Vec<String>, usize> = HashMap::new();
            for row in 0..ctx.row_count {
                if !allowed.get(row).copied().unwrap_or(false) { continue; }
                let mut key = Vec::new();
                let mut empty = false;
                for c in columns {
                    let class = class_of(ctx.columns, c).unwrap_or(ValueClass::Text);
                    let nv = class.normalize(&cell_value(ctx, row, c));
                    if nv.is_empty() { empty = true; break; }
                    key.push(nv);
                }
                if empty { continue; }
                if let Some(&first) = seen.get(&key) {
                    push_error(push, ctx, ValidationCode::RuleTableViolation, row,
                        &columns[0], columns.clone(),
                        format!("列组合 ({}) 与第 {} 行重复", key.join(", "), first + 1),
                        &key.join(", "));
                } else {
                    seen.insert(key, row);
                }
            }
        }
        RuleKind::RowCount { min, max } => {
            let n = (0..ctx.row_count)
                .filter(|&r| allowed.get(r).copied().unwrap_or(false) && !(ctx.row_is_empty)(r))
                .count() as f64;
            let lo = parse_bound(min).ok().flatten();
            let hi = parse_bound(max).ok().flatten();
            let bad = lo.map_or(false, |b| n < b) || hi.map_or(false, |b| n > b);
            if !bad { return; }
            let range = match (lo, hi) {
                (Some(a), Some(b)) => format!("[{}, {}]", a, b),
                (Some(a), None) => format!("≥ {}", a),
                (None, Some(b)) => format!("≤ {}", b),
                (None, None) => String::new(),
            };
            push_error(push, ctx, ValidationCode::RuleTableViolation, TABLE_ROW,
                "", Vec::new(),
                format!("数据行数 {} 不在允许区间 {} 内", n as i64, range), "");
        }
        RuleKind::IdContiguous => {
            let mut ids = Vec::new();
            for row in 0..ctx.row_count {
                if !allowed.get(row).copied().unwrap_or(false) { continue; }
                if (ctx.row_is_empty)(row) { continue; }
                let raw = cell_value(ctx, row, "id");
                if raw.trim().is_empty() { continue; }
                match raw.trim().parse::<i64>() {
                    Ok(n) => ids.push((row, n)),
                    Err(_) => {
                        push_error(push, ctx, ValidationCode::RuleTableViolation, row, "id",
                            vec!["id".into()],
                            format!("id \"{}\" 不是整数", raw.trim()), raw.trim());
                    }
                }
            }
            let n = ids.len() as i64;
            let mut seen = HashSet::new();
            for (row, id) in &ids {
                if *id < 1 || *id > n || !seen.insert(*id) {
                    push_error(push, ctx, ValidationCode::RuleTableViolation, *row, "id",
                        vec!["id".into()],
                        format!("id 必须恰好构成 1..={} 的连续集合（发现 {}）", n, id),
                        &id.to_string());
                }
            }
            if seen.len() as i64 != n {
                let missing: Vec<String> = (1..=n)
                    .filter(|x| !seen.contains(x))
                    .map(|x| x.to_string())
                    .collect();
                push_error(push, ctx, ValidationCode::RuleTableViolation, TABLE_ROW, "",
                    vec!["id".into()],
                    format!("id 集合缺少：{}", missing.join(", ")), "");
            }
        }
        RuleKind::NoEmptyRows => {}
        _ => {}
    }
}

fn eval_condition<F>(
    cond: &Condition,
    row: usize,
    columns: &[ColumnSpec],
    cell: &F,
) -> bool
where
    F: Fn(usize, &str) -> Option<String>,
{
    match cond {
        Condition::And(cs) => cs.iter().all(|c| eval_condition(c, row, columns, cell)),
        Condition::Or(cs) => cs.iter().any(|c| eval_condition(c, row, columns, cell)),
        Condition::Not(c) => !eval_condition(c, row, columns, cell),
        Condition::Cmp { left, op, right } => {
            let (lv, lc) = eval_value(left, row, columns, cell);
            let (rv, rc) = eval_value(right, row, columns, cell);
            match op {
                CompareOp::Eq | CompareOp::Ne => {
                    let a = lc.unwrap_or(ValueClass::Text).normalize(&lv);
                    let b = rc.unwrap_or(ValueClass::Text).normalize(&rv);
                    if *op == CompareOp::Eq { a == b } else { a != b }
                }
                _ => {
                    let (Ok(a), Ok(b)) = (lv.trim().parse::<f64>(), rv.trim().parse::<f64>()) else {
                        return false;
                    };
                    match op {
                        CompareOp::Lt => a < b,
                        CompareOp::Le => a <= b,
                        CompareOp::Gt => a > b,
                        CompareOp::Ge => a >= b,
                        _ => false,
                    }
                }
            }
        }
        Condition::In { value, values } => {
            let (v, vc) = eval_value(value, row, columns, cell);
            let class = vc.unwrap_or(ValueClass::Text);
            let nv = class.normalize(&v);
            values.iter().any(|e| {
                let (x, xc) = eval_value(e, row, columns, cell);
                xc.unwrap_or(class).normalize(&x) == nv
            })
        }
        Condition::Match { value, pattern } => {
            let (v, _) = eval_value(value, row, columns, cell);
            regex::Regex::new(pattern).map_or(false, |re| re.is_match(&v))
        }
    }
}

fn eval_value<F>(v: &ValueExpr, row: usize, columns: &[ColumnSpec], cell: &F)
    -> (String, Option<ValueClass>)
where
    F: Fn(usize, &str) -> Option<String>,
{
    match v {
        ValueExpr::Literal(s) => (s.clone(), None),
        ValueExpr::Column(name) => {
            let class = class_of(columns, name);
            (cell(row, name).unwrap_or_default(), class)
        }
    }
}

/// 节点级规则校验入口：schema + 数据行。
pub fn validate_node_rules<F, G>(
    mode: NodeMode,
    node_name: &str,
    columns: &[ColumnSpec],
    rules: &[Rule],
    cidx: &RuleIndex,
    row_count: usize,
    cell: F,
    row_is_empty: G,
) -> Vec<ValidationError>
where
    F: Fn(usize, &str) -> Option<String>,
    G: Fn(usize) -> bool,
{
    let mut errors = validate_rules_schema(mode, node_name, columns, rules, cidx);
    for (rule_idx, rule) in rules.iter().enumerate() {
        if rule.source == crate::rules::RuleSource::User
            && rule.conflicts_with_builtin(mode)
        {
            continue;
        }
        if rule_schema_error(rule, mode, node_name, columns, cidx).is_some() {
            continue;
        }
        let ctx = EvalCtx {
            columns,
            cidx,
            row_count,
            cell: &cell,
            row_is_empty: &row_is_empty,
            level: rule.level,
            rule_index: rule_idx,
        };
        let allowed = vec![true; row_count];
        eval_expr(&ctx, &rule.expr, &allowed, &mut |e| errors.push(e));
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleSource;
    use crate::schema_grammar::parse_rule_body;

    fn cols(items: &[(&str, Option<ValueClass>)]) -> Vec<ColumnSpec> {
        items.iter().map(|(n, c)| (n.to_string(), *c)).collect()
    }

    fn rule(body: &str) -> Rule {
        parse_rule_body(body).expect("parse rule")
    }

    fn run(columns: &[ColumnSpec], rules: &[Rule], rows: &[Vec<&str>]) -> Vec<ValidationError> {
        let idx = RuleIndex::build_schema(&[]);
        validate_node_rules(
            NodeMode::Table, "T", columns, rules, &idx, rows.len(),
            |r, c| {
                let i = columns.iter().position(|(n, _)| n == c)?;
                rows.get(r)?.get(i).map(|s| s.to_string())
            },
            |r| rows.get(r).map_or(true, |row| row.iter().all(|v| v.trim().is_empty())),
        )
    }

    fn item_cols() -> Vec<ColumnSpec> {
        cols(&[
            ("id", Some(ValueClass::Int)),
            ("type", Some(ValueClass::Int)),
            ("quality", Some(ValueClass::Int)),
            ("value", Some(ValueClass::Text)),
            ("price", Some(ValueClass::Int)),
            ("code", Some(ValueClass::Text)),
        ])
    }

    fn body(rule: &str) -> RuleExpr {
        crate::schema_grammar::parse_rule_body(rule).expect("parse").expr
    }
    fn _keep_source(_: RuleSource) {}

    #[test]
    fn column_when_and_regex() {
        let columns = item_cols();
        let r = rule(r#"column value: when(and(eq(type, 1), ge(quality, 2)), matches("^\d+$"))"#);
        let errs = run(&columns, &[r], &[
            vec!["1", "1", "5", "abc", "1", ""], // 命中且非法
            vec!["2", "1", "1", "abc", "1", ""], // quality 不满足
            vec!["3", "2", "5", "abc", "1", ""], // type 不满足
            vec!["4", "1", "5", "123", "1", ""], // 命中且合法
        ]);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].row, 0);
        assert_eq!(errs[0].field, "value");
    }

    #[test]
    fn table_row_functions_report_rows_and_columns() {
        let columns = cols(&[
            ("id", Some(ValueClass::Int)),
            ("value", Some(ValueClass::Text)),
            ("price", Some(ValueClass::Int)),
        ]);
        let errs = run(
            &columns,
            &[rule("table: all_non_empty(value, price)")],
            &[
                vec!["1", "a", "1"],
                vec!["2", "", "1"],
                vec!["3", "a", ""],
                vec!["", "", ""], // 整行占位行，跳过
            ],
        );
        assert_eq!(errs.len(), 2, "{errs:?}");
        assert_eq!(errs.iter().map(|e| e.row).collect::<Vec<_>>(), vec![1, 2]);
        assert!(errs[0].affected.contains(&"value".to_string()));
        assert!(errs[0].affected.contains(&"price".to_string()));

        let errs = run(
            &columns,
            &[rule("table: exactly_one(value, price)")],
            &[
                vec!["1", "a", ""],  // 恰好一列 → 通过
                vec!["2", "a", "1"], // 两列都有 → 失败
                vec!["", "", ""],    // 占位行 → 跳过
            ],
        );
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].row, 1);
    }

    #[test]
    fn table_row_count_and_id_contiguous() {
        let columns = item_cols();
        let errs = run(
            &columns,
            &[rule("table: row_count(5, 10)")],
            &[vec!["1", "1", "1", "a", "1", ""], vec!["2", "1", "1", "b", "1", ""]],
        );
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert!(errs[0].is_table_level());
        assert_eq!(errs[0].code, ValidationCode::RuleTableViolation);

        let errs = run(
            &columns,
            &[rule("table: id_contiguous()")],
            &[
                vec!["1", "1", "1", "a", "1", ""],
                vec!["3", "1", "1", "b", "1", ""],
            ],
        );
        assert!(!errs.is_empty(), "1,3 不连续应报错");
        assert!(errs.iter().any(|e| e.code == ValidationCode::RuleTableViolation));
    }

    #[test]
    fn unique_under_when_filters_rows() {
        let columns = item_cols();
        let r = rule("column code: when(eq(type, 1), unique())");
        let errs = run(&columns, &[r], &[
            vec!["1", "1", "1", "a", "1", "A"],
            vec!["2", "1", "1", "b", "1", "A"], // 条件内重复
            vec!["3", "2", "1", "c", "1", "A"], // 条件外，不参与
            vec!["4", "2", "1", "d", "1", "A"],
        ]);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].row, 1);
        assert_eq!(errs[0].code, ValidationCode::RuleUniqueViolation);
    }

    #[test]
    fn unique_combination_reports_duplicate_row() {
        let columns = item_cols();
        let errs = run(
            &columns,
            &[rule("table: unique_combination(type, quality)")],
            &[
                vec!["1", "1", "2", "a", "1", ""],
                vec!["2", "1", "2", "b", "1", ""], // 重复组合
                vec!["3", "1", "3", "c", "1", ""],
            ],
        );
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].row, 1);
    }

    #[test]
    fn or_row_functions_need_any_to_pass() {
        let columns = item_cols();
        let r = rule("table: or(non_empty(value), non_empty(code))");
        let errs = run(&columns, &[r], &[
            vec!["1", "1", "1", "", "1", ""],   // 两个都空 → 失败
            vec!["2", "1", "1", "a", "1", ""],  // value 有值 → 通过
            vec!["3", "1", "1", "", "1", "X"],  // code 有值 → 通过
        ]);
        assert_eq!(errs.len(), 1, "{errs:?}");
        assert_eq!(errs[0].row, 0);
    }

    #[test]
    fn when_can_filter_table_functions() {
        let columns = item_cols();
        let r = rule("table: when(eq(type, 1), row_count(5, 10))");
        let errs = run(&columns, &[r], &[
            vec!["1", "1", "1", "a", "1", ""],
            vec!["2", "2", "1", "b", "1", ""],
            vec!["3", "2", "1", "c", "1", ""],
        ]);
        assert_eq!(errs.len(), 1, "type=1 只有 1 行，低于 5");
        assert!(errs[0].is_table_level());
    }

    #[test]
    fn schema_scope_and_type_errors() {
        let columns = item_cols();
        let e = parse_rule_body("column value: all_non_empty(value, price)").unwrap_err();
        assert!(e.contains("不能用在列级"), "{e}");

        let errs = run(&columns, &[rule("table: range(value, 1, 10)")], &[]);
        assert!(errs.iter().any(|e| e.code == ValidationCode::RuleTypeIncompatible),
            "{errs:?}");

        let errs = run(&columns, &[rule("table: or(row_count(1, 2), id_contiguous())")], &[]);
        assert!(errs.iter().any(|e| e.code == ValidationCode::RuleParamInvalid),
            "{errs:?}");
    }

    #[test]
    fn rename_touches_all_rule_columns() {
        let mut r = rule("table: and(non_empty(value), compare(value, <=, price))");
        r.rename_column("value", "amount");
        assert!(r.body_string().contains("non_empty(amount)"), "{}", r.body_string());
        assert!(r.body_string().contains("compare(amount, <=, price)"), "{}", r.body_string());
    }

    #[test]
    fn _body_helper_is_used() {
        let _ = body("column value: non_empty()");
    }
}
