//! 校验规则面板（@01.10.7 / @16）：列规则 / 表级规则两个 Tab + 规则表达式树。
//!
//! 规则不是「条件卡 + 验证器表单」，而是一棵可增删的组/节点树：
//! - 规则组（AND / OR）可以嵌套；
//! - `when` 是可选节点，里面分「条件组」和「验证组」两条 lane；
//! - 验证器、条件是叶子节点；验证器参数在选中后的 inspector 里编辑。
//!
//! 添加验证器时打开函数选择器：按业务层分组，带签名、说明、适用列/粒度，
//! 选中后在右侧看参数说明与示例，再点「添加」。

use std::cell::RefCell;
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use tablet_core::model::{
    builtin_rules, CompareOp, Condition, NodeMode, Rule, RuleExpr, RuleKind, RuleScope,
    RuleSource, RuleTarget, Severity, ValueExpr,
};
use tablet_core::ops::format_validation_log;
use tablet_core::rule_engine::{rule_params_valid, validate_rules_schema};
use tablet_core::validate::{
    validate_enum, validate_table, ColumnSpec, RefIndex, RuleIndex, ValidationError, ValueClass,
};

use crate::state::{AppState, SelectedNode};
use crate::{
    AppWindow, PickerParam, RuleCard, RuleColumnRow, RuleFunctionItem, RuleIssueRow, RuleTreeRow,
};

const DESC_UNIQUE: &str = "本列的非空值在节点内不能重复；整数列按数值比较（01 与 1 视为相同）。";
const DESC_REF_EXISTS: &str = "本列每个非空值都必须出现在目标节点的目标列中。";
const DESC_NON_EMPTY: &str = "该列必须有值；整行全空的占位行不算缺填。";
const DESC_RANGE: &str = "数值必须落在区间内；留空一侧表示不限。";
const DESC_LEN: &str = "按字符数限制长度；留空一侧表示不限。";
const DESC_ONE_OF: &str = "值必须是候选之一；多个候选用逗号分隔，含逗号的值加引号。";
const DESC_MATCHES: &str = "整串匹配请写 ^...$；否则是按「包含」匹配。";
const DESC_COMPARE: &str = "与另一列按运算符比较（两边都必须能解析为数字）。";
const DESC_POSITIVE_INT: &str = "必须是正整数（不能为 0 或负数）。";
const DESC_UPPER_SNAKE: &str = "必须是 UPPER_SNAKE_CASE（大写字母、数字、下划线）。";
const DESC_ALL_NON_EMPTY: &str = "这些列都必须有值（逗号分隔）；整行全空的占位行不算。";
const DESC_AT_LEAST_ONE: &str = "这些列至少有一列有值（逗号分隔）。";
const DESC_EXACTLY_ONE: &str = "这些列恰好有一列有值（逗号分隔）。";
const DESC_ROW_COUNT: &str = "非占位行数必须落在区间内；留空一侧不限。";
const DESC_ID_CONTIGUOUS: &str = "id 集合必须恰好是 1..=N（N = 非占位行数）。";
const DESC_UNIQUE_COMBINATION: &str = "这些列（逗号分隔）的取值组合在表内不能重复。";
const DESC_UNSUPPORTED: &str =
    "该列是复合类型（List / Set / Map / Tuple 等），当前版本不参与校验规则。";
const HINT_DUPLICATE: &str = "该作用域已有等价规则（同类型、同目标），不能重复添加。";
const HINT_INCOMPLETE: &str =
    "规则不完整：至少需要一个验证器；条件过滤至少要有一个条件；组不能为空。";

const COLUMN_VALIDATORS: &[&str] = &[
    "unique", "non_empty", "ref_exists", "range", "len", "one_of", "matches",
    "compare", "positive_int", "upper_snake",
];
const ROW_VALIDATORS: &[&str] = &["all_non_empty", "at_least_one", "exactly_one"];
const TABLE_VALIDATORS: &[&str] = &["row_count", "id_contiguous", "unique_combination"];

fn validator_keys(scope: RuleScope) -> Vec<&'static str> {
    match scope {
        RuleScope::Column => COLUMN_VALIDATORS.to_vec(),
        RuleScope::Table => COLUMN_VALIDATORS
            .iter()
            .chain(ROW_VALIDATORS.iter())
            .chain(TABLE_VALIDATORS.iter())
            .copied()
            .collect(),
    }
}

fn is_column_domain(key: &str) -> bool {
    COLUMN_VALIDATORS.contains(&key)
}

fn validator_applies(key: &str, class: Option<ValueClass>) -> bool {
    match key {
        "range" | "compare" => matches!(class, Some(ValueClass::Int) | Some(ValueClass::Float)),
        "len" | "matches" | "upper_snake" => matches!(class, Some(ValueClass::Text)),
        "positive_int" => matches!(class, Some(ValueClass::Int)),
        _ => class.is_some(),
    }
}

const CONDITION_KEYS: &[&str] = &["eq", "ne", "lt", "le", "gt", "ge", "in", "match"];

fn condition_label(key: &str) -> &'static str {
    match key {
        "eq" => "条件: 等于",
        "ne" => "条件: 不等于",
        "lt" => "条件: 小于",
        "le" => "条件: 小于等于",
        "gt" => "条件: 大于",
        "ge" => "条件: 大于等于",
        "in" => "条件: 属于",
        "match" => "条件: 匹配正则",
        _ => "条件",
    }
}

fn default_condition(key: &str, specs: &[ColumnSpec], anchor: Option<&str>) -> LeafCond {
    let mut leaf = default_leaf(specs, anchor);
    match key {
        "ne" => leaf.op = LeafOp::Ne,
        "lt" => leaf.op = LeafOp::Lt,
        "le" => leaf.op = LeafOp::Le,
        "gt" => leaf.op = LeafOp::Gt,
        "ge" => leaf.op = LeafOp::Ge,
        "in" => leaf.op = LeafOp::In,
        "match" => leaf.op = LeafOp::Match,
        _ => leaf.op = LeafOp::Eq,
    }
    if leaf.op == LeafOp::Match {
        leaf.pattern.clear();
    }
    leaf
}

fn validator_label(key: &str) -> &'static str {
    match key {
        "unique" => "unique()",
        "non_empty" => "non_empty()",
        "range" => "range(下限, 上限)",
        "len" => "len(下限, 上限)",
        "one_of" => "one_of(候选值)",
        "matches" => "matches(正则)",
        "ref_exists" => "ref_exists(节点.列)",
        "compare" => "compare(运算符, 比较列)",
        "positive_int" => "positive_int()",
        "upper_snake" => "upper_snake()",
        "all_non_empty" => "all_non_empty(列...)",
        "at_least_one" => "at_least_one(列...)",
        "exactly_one" => "exactly_one(列...)",
        "row_count" => "row_count(下限, 上限)",
        "id_contiguous" => "id_contiguous()",
        "unique_combination" => "unique_combination(列...)",
        _ => "?",
    }
}

fn validator_desc(key: &str) -> &'static str {
    match key {
        "unique" => DESC_UNIQUE,
        "non_empty" => DESC_NON_EMPTY,
        "range" => DESC_RANGE,
        "len" => DESC_LEN,
        "one_of" => DESC_ONE_OF,
        "matches" => DESC_MATCHES,
        "ref_exists" => DESC_REF_EXISTS,
        "compare" => DESC_COMPARE,
        "positive_int" => DESC_POSITIVE_INT,
        "upper_snake" => DESC_UPPER_SNAKE,
        "all_non_empty" => DESC_ALL_NON_EMPTY,
        "at_least_one" => DESC_AT_LEAST_ONE,
        "exactly_one" => DESC_EXACTLY_ONE,
        "row_count" => DESC_ROW_COUNT,
        "id_contiguous" => DESC_ID_CONTIGUOUS,
        "unique_combination" => DESC_UNIQUE_COMBINATION,
        _ => "",
    }
}

fn validator_tags(key: &str) -> &'static str {
    match key {
        "unique" => "全部标量 · 全表 · 列级/表级",
        "non_empty" => "全部标量 · 逐行 · 列级/表级",
        "ref_exists" => "全部标量 · 逐行 · 列级/表级",
        "range" => "整数/浮点 · 逐行 · 列级/表级",
        "len" => "文本 · 逐行 · 列级/表级",
        "one_of" => "全部标量 · 逐行 · 列级/表级",
        "matches" => "文本 · 逐行 · 列级/表级",
        "compare" => "整数/浮点 · 逐行 · 列级/表级",
        "positive_int" => "整数 · 逐行 · 列级/表级",
        "upper_snake" => "文本 · 逐行 · 列级/表级",
        "all_non_empty" => "≥ 2 列 · 逐行 · 仅表级",
        "at_least_one" => "≥ 2 列 · 逐行 · 仅表级",
        "exactly_one" => "≥ 2 列 · 逐行 · 仅表级",
        "row_count" => "全表 · 仅表级",
        "id_contiguous" => "全表 · 仅表级",
        "unique_combination" => "≥ 2 列 · 全表 · 仅表级",
        _ => "",
    }
}

fn picker_signature(key: &str, scope: RuleScope) -> &'static str {
    if scope == RuleScope::Column {
        return match key {
            "unique" => "unique()",
            "non_empty" => "non_empty()",
            "range" => "range(min, max)",
            "len" => "len(min, max)",
            "one_of" => "one_of(v1, v2, ...)",
            "matches" => "matches(\"正则\")",
            "ref_exists" => "ref_exists(节点.列)",
            "compare" => "compare(op, other)",
            "positive_int" => "positive_int()",
            "upper_snake" => "upper_snake()",
            "all_non_empty" => "all_non_empty(col1, col2, ...)",
            "at_least_one" => "at_least_one(col1, col2, ...)",
            "exactly_one" => "exactly_one(col1, col2, ...)",
            "row_count" => "row_count(min, max)",
            "id_contiguous" => "id_contiguous()",
            "unique_combination" => "unique_combination(col1, ...)",
            _ => "?",
        };
    }
    match key {
        "unique" => "unique(col)",
        "non_empty" => "non_empty(col)",
        "range" => "range(col, min, max)",
        "len" => "len(col, min, max)",
        "one_of" => "one_of(col, v1, v2, ...)",
        "matches" => "matches(col, \"正则\")",
        "ref_exists" => "ref_exists(col, 节点.列)",
        "compare" => "compare(col, op, other)",
        "positive_int" => "positive_int(col)",
        "upper_snake" => "upper_snake(col)",
        "all_non_empty" => "all_non_empty(col1, col2, ...)",
        "at_least_one" => "at_least_one(col1, col2, ...)",
        "exactly_one" => "exactly_one(col1, col2, ...)",
        "row_count" => "row_count(min, max)",
        "id_contiguous" => "id_contiguous()",
        "unique_combination" => "unique_combination(col1, ...)",
        _ => "?",
    }
}

fn picker_params(key: &str, scope: RuleScope) -> Vec<PickerParam> {
    let p = |name: &str, kind: &str, desc: &str| PickerParam {
        name: SharedString::from(name),
        kind: SharedString::from(kind),
        desc: SharedString::from(desc),
    };
    let mut out = Vec::new();
    if scope == RuleScope::Table && is_column_domain(key) {
        out.push(p("col", "列", "主列；表级规则显式选择"));
    }
    match key {
        "range" | "len" | "row_count" => {
            out.push(p("min", "number", "下限，可空"));
            out.push(p("max", "number", "上限，可空"));
        }
        "one_of" => out.push(p("v1, v2, ...", "标量", "候选值；含逗号的值加引号")),
        "matches" => out.push(p("正则", "text", "整串匹配写 ^...$")),
        "ref_exists" => out.push(p("节点.列", "节点.列", "引用的目标节点与目标列")),
        "compare" => {
            out.push(p("op", "运算符", "< <= > >= == !="));
            out.push(p("other", "列", "与本列比较的另一列；两边都要是数字"));
        }
        "all_non_empty" | "at_least_one" | "exactly_one" | "unique_combination" =>
            out.push(p("col1, col2, ...", "列列表", "至少 2 个标量列")),
        _ => {}
    }
    out
}

fn picker_example(key: &str, scope: RuleScope) -> String {
    let body = match key {
        "unique" => "unique()",
        "non_empty" => "non_empty()",
        "range" => if scope == RuleScope::Column { "range(0, 100)" } else { "range(hp, 0, 100)" },
        "len" => if scope == RuleScope::Column { "len(1, 64)" } else { "len(name, 1, 64)" },
        "one_of" => if scope == RuleScope::Column { "one_of(1, 2, 3)" } else { "one_of(type, 1, 2, 3)" },
        "matches" => if scope == RuleScope::Column { "matches(\"^[A-Z_]+$\")" } else { "matches(name, \"^[A-Z_]+$\")" },
        "ref_exists" => if scope == RuleScope::Column { "ref_exists(Lang.key)" } else { "ref_exists(kind, HeroType.id)" },
        "compare" => if scope == RuleScope::Column { "compare(<=, hp)" } else { "compare(atk, <=, hp)" },
        "positive_int" => "positive_int()",
        "upper_snake" => "upper_snake()",
        "all_non_empty" => "all_non_empty(name, hp)",
        "at_least_one" => "at_least_one(name, title)",
        "exactly_one" => "exactly_one(damage, heal)",
        "row_count" => "row_count(1, 100)",
        "id_contiguous" => "id_contiguous()",
        "unique_combination" => "unique_combination(kind, quality)",
        _ => "",
    };
    match scope {
        RuleScope::Column => format!("column value: {}", body),
        RuleScope::Table => format!("table: {}", body),
    }
}

// ── 条件树（GUI 草稿）──────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LogicOp { And, Or }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeafOp { Eq, Ne, Lt, Le, Gt, Ge, In, Match }

#[derive(Debug, Clone)]
pub(crate) struct LeafCond {
    column: String,
    op: LeafOp,
    value: String,
    values: Vec<String>,
    pattern: String,
}

impl LogicOp {
    fn index(self) -> i32 { if self == LogicOp::Or { 1 } else { 0 } }
    fn from_label(label: &str) -> Self { if label.contains("OR") { LogicOp::Or } else { LogicOp::And } }
}

impl LeafOp {
    fn label(self) -> &'static str {
        match self {
            LeafOp::Eq => "等于", LeafOp::Ne => "不等于", LeafOp::Lt => "小于",
            LeafOp::Le => "小于等于", LeafOp::Gt => "大于", LeafOp::Ge => "大于等于",
            LeafOp::In => "属于", LeafOp::Match => "匹配正则",
        }
    }
    fn parse_label(label: &str) -> Option<Self> {
        [LeafOp::Eq, LeafOp::Ne, LeafOp::Lt, LeafOp::Le, LeafOp::Gt, LeafOp::Ge,
         LeafOp::In, LeafOp::Match]
            .into_iter().find(|op| op.label() == label)
    }
    fn value_mode(self) -> &'static str {
        match self { LeafOp::In => "in", LeafOp::Match => "match", _ => "single" }
    }
    fn compare(self) -> Option<CompareOp> {
        match self {
            LeafOp::Eq => Some(CompareOp::Eq), LeafOp::Ne => Some(CompareOp::Ne),
            LeafOp::Lt => Some(CompareOp::Lt), LeafOp::Le => Some(CompareOp::Le),
            LeafOp::Gt => Some(CompareOp::Gt), LeafOp::Ge => Some(CompareOp::Ge),
            _ => None,
        }
    }
    fn from_compare(op: CompareOp) -> Self {
        match op {
            CompareOp::Eq => LeafOp::Eq, CompareOp::Ne => LeafOp::Ne,
            CompareOp::Lt => LeafOp::Lt, CompareOp::Le => LeafOp::Le,
            CompareOp::Gt => LeafOp::Gt, CompareOp::Ge => LeafOp::Ge,
        }
    }
}

fn scalar_columns(specs: &[ColumnSpec]) -> Vec<String> {
    specs.iter().filter(|(_, c)| c.is_some()).map(|(n, _)| n.clone()).collect()
}

fn column_class(specs: &[ColumnSpec], name: &str) -> Option<ValueClass> {
    specs.iter().find(|(n, _)| n == name).and_then(|(_, c)| *c)
}

fn leaf_op_options(class: Option<ValueClass>) -> Vec<String> {
    let ops: &[LeafOp] = match class {
        Some(ValueClass::Int) | Some(ValueClass::Float) => &[
            LeafOp::Eq, LeafOp::Ne, LeafOp::Lt, LeafOp::Le, LeafOp::Gt, LeafOp::Ge, LeafOp::In,
        ],
        _ => &[LeafOp::Eq, LeafOp::Ne, LeafOp::In, LeafOp::Match],
    };
    ops.iter().map(|o| o.label().to_string()).collect()
}

fn leaf_to_condition(l: &LeafCond) -> Option<Condition> {
    if l.column.trim().is_empty() { return None; }
    let left = ValueExpr::Column(l.column.clone());
    match l.op {
        LeafOp::In => {
            let values: Vec<ValueExpr> = l.values.iter()
                .filter(|v| !v.trim().is_empty())
                .map(|v| ValueExpr::Literal(v.clone()))
                .collect();
            if values.is_empty() { None } else { Some(Condition::In { value: left, values }) }
        }
        LeafOp::Match => {
            if l.pattern.trim().is_empty() { None }
            else { Some(Condition::Match { value: left, pattern: l.pattern.clone() }) }
        }
        _ => {
            let op = l.op.compare()?;
            Some(Condition::Cmp { left, op, right: ValueExpr::Literal(l.value.clone()) })
        }
    }
}

fn condition_to_leaf(cond: &Condition) -> Result<LeafCond, String> {
    match cond {
        Condition::Cmp { left, op, right } => {
            let ValueExpr::Column(col) = left else { return Err("条件左侧不是列".into()) };
            let ValueExpr::Literal(val) = right else {
                return Err("列与列比较暂不支持图形编辑".into());
            };
            Ok(LeafCond {
                column: col.clone(),
                op: LeafOp::from_compare(*op),
                value: val.clone(),
                values: vec![val.clone()],
                pattern: String::new(),
            })
        }
        Condition::In { value, values } => {
            let ValueExpr::Column(col) = value else { return Err("in 左侧不是列".into()) };
            let mut out = Vec::new();
            for v in values {
                let ValueExpr::Literal(s) = v else {
                    return Err("条件里的列引用暂不支持图形编辑".into());
                };
                out.push(s.clone());
            }
            Ok(LeafCond {
                column: col.clone(),
                op: LeafOp::In,
                value: out.first().cloned().unwrap_or_default(),
                values: out,
                pattern: String::new(),
            })
        }
        Condition::Match { value, pattern } => {
            let ValueExpr::Column(col) = value else { return Err("match 左侧不是列".into()) };
            Ok(LeafCond {
                column: col.clone(),
                op: LeafOp::Match,
                value: String::new(),
                values: Vec::new(),
                pattern: pattern.clone(),
            })
        }
        Condition::And(_) | Condition::Or(_) | Condition::Not(_) =>
            Err("条件组需要作为组处理".into()),
    }
}

fn group_from_conditions(cs: &[Condition], op: LogicOp) -> Result<DraftExpr, String> {
    let children = cs.iter().map(condition_to_draft).collect::<Result<Vec<_>, _>>()?;
    let conns = vec![op; children.len()];
    Ok(group_with(children, conns))
}

fn condition_to_draft(cond: &Condition) -> Result<DraftExpr, String> {
    match cond {
        Condition::Cmp { .. } | Condition::In { .. } | Condition::Match { .. } =>
            Ok(DraftExpr::Condition(condition_to_leaf(cond)?)),
        Condition::And(cs) => group_from_conditions(cs, LogicOp::And),
        Condition::Or(cs) => group_from_conditions(cs, LogicOp::Or),
        Condition::Not(_) =>
            Err("not(...) 条件暂不支持图形编辑，请在 schema 中修改".into()),
    }
}

fn is_condition_only(node: &DraftExpr) -> bool {
    match node {
        DraftExpr::Condition(_) => true,
        DraftExpr::Group { children, .. } if !children.is_empty() =>
            children.iter().all(is_condition_only),
        _ => false,
    }
}

fn and_conditions(mut items: Vec<Condition>) -> Condition {
    if items.len() == 1 { items.remove(0) } else { Condition::And(items) }
}

/// 组内按成员顺序拼接条件：AND 优先，OR 分段。
fn combine_conditions(items: Vec<(Condition, LogicOp)>) -> Option<Condition> {
    if items.is_empty() { return None; }
    let mut terms = Vec::new();
    let mut and_items = Vec::new();
    for (cond, conn) in items {
        and_items.push(cond);
        if conn == LogicOp::Or {
            terms.push(and_conditions(std::mem::take(&mut and_items)));
        }
    }
    if !and_items.is_empty() { terms.push(and_conditions(and_items)); }
    match terms.len() {
        0 => None,
        1 => Some(terms.remove(0)),
        _ => Some(Condition::Or(terms)),
    }
}

fn condition_of(node: &DraftExpr) -> Option<Condition> {
    match node {
        DraftExpr::Condition(l) => leaf_to_condition(l),
        DraftExpr::Group { children, conns } => {
            let mut items = Vec::with_capacity(children.len());
            for (i, child) in children.iter().enumerate() {
                items.push((condition_of(child)?, conn_at(conns, i)));
            }
            combine_conditions(items)
        }
        DraftExpr::Validator(_) => None,
    }
}

fn merge_when(cond: DraftExpr, inner: DraftExpr) -> DraftExpr {
    let (mut cc, mut ck) = match cond {
        DraftExpr::Group { children, conns } => (children, conns),
        other => (vec![other], vec![LogicOp::And]),
    };
    let (mut ic, mut ik) = match inner {
        DraftExpr::Group { children, conns } => (children, conns),
        other => (vec![other], vec![LogicOp::And]),
    };
    if cc.is_empty() { return group_with(ic, ik); }
    if ic.is_empty() { return group_with(cc, ck); }
    if let Some(last) = ck.last_mut() { *last = LogicOp::And; }
    cc.append(&mut ic);
    ck.append(&mut ik);
    group_with(cc, ck)
}

fn default_leaf(specs: &[ColumnSpec], anchor: Option<&str>) -> LeafCond {
    let columns = scalar_columns(specs);
    let column = columns.iter()
        .find(|c| Some(c.as_str()) != anchor)
        .cloned()
        .or_else(|| columns.first().cloned())
        .unwrap_or_default();
    LeafCond {
        column, op: LeafOp::Eq, value: "1".into(),
        values: vec!["1".into()], pattern: String::new(),
    }
}

// ── 规则草稿树（混合组：条件行 + 验证器行）─────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum DraftExpr {
    /// `conns[i]` 是 children[i] 与下一名成员之间的连接符；最后一项的连接符不使用。
    /// 组本身对外的连接符存放在父组的 conns 里。
    Group { children: Vec<DraftExpr>, conns: Vec<LogicOp> },
    Validator(RuleKind),
    Condition(LeafCond),
}

fn conn_at(conns: &[LogicOp], i: usize) -> LogicOp {
    conns.get(i).copied().unwrap_or(LogicOp::And)
}

fn group_with(children: Vec<DraftExpr>, conns: Vec<LogicOp>) -> DraftExpr {
    debug_assert_eq!(children.len(), conns.len(), "conns must align with children");
    DraftExpr::Group { children, conns }
}

/// 一行规则树节点：指向第几条规则草稿、以及该草稿内的路径。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RowKey {
    pub rule: usize,
    pub path: Vec<usize>,
}

struct TreeRowData {
    row: RuleTreeRow,
    key: RowKey,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum EditorMode {
    Advanced,
}

fn empty_group() -> DraftExpr {
    DraftExpr::Group { children: Vec::new(), conns: Vec::new() }
}

/// 简单模式只能表达「根组 + 平铺验证器 + 全部 AND」；条件、OR、嵌套组都算高级。
#[cfg(test)]
fn simple_supported(draft: &DraftExpr) -> bool {
    let normalized = normalize_draft(draft.clone());
    matches!(
        normalized,
        DraftExpr::Group { children, conns }
            if children.iter().all(|c| matches!(c, DraftExpr::Validator(_)))
                && conns.iter().all(|c| *c == LogicOp::And)
    )
}

fn normalize_draft(draft: DraftExpr) -> DraftExpr {
    match draft {
        DraftExpr::Group { children, conns } => {
            let (children, conns) = normalize_children(children, &conns);
            group_with(children, conns)
        }
        other => group_with(vec![other], vec![LogicOp::And]),
    }
}

fn normalize_children(
    children: Vec<DraftExpr>,
    conns: &[LogicOp],
) -> (Vec<DraftExpr>, Vec<LogicOp>) {
    let mut out = Vec::with_capacity(children.len());
    let mut out_conns = Vec::with_capacity(children.len());
    for (i, child) in children.into_iter().enumerate() {
        out.push(normalize_node(child));
        out_conns.push(conn_at(conns, i));
    }
    (out, out_conns)
}

fn normalize_node(node: DraftExpr) -> DraftExpr {
    match node {
        DraftExpr::Group { children, conns } => {
            let (mut out, out_conns) = normalize_children(children, &conns);
            if out.len() == 1 {
                out.remove(0)
            } else {
                group_with(out, out_conns)
            }
        }
        other => other,
    }
}

fn draft_from_expr(expr: &RuleExpr) -> Result<DraftExpr, String> {
    let node = match expr {
        RuleExpr::Validator(v) => DraftExpr::Validator(v.clone()),
        RuleExpr::And(cs) => {
            let children = cs.iter().map(draft_from_expr).collect::<Result<Vec<_>, _>>()?;
            let conns = vec![LogicOp::And; children.len()];
            group_with(children, conns)
        }
        RuleExpr::Or(cs) => {
            let children = cs.iter().map(draft_from_expr).collect::<Result<Vec<_>, _>>()?;
            let conns = vec![LogicOp::Or; children.len()];
            group_with(children, conns)
        }
        RuleExpr::When { condition, inner } => merge_when(
            condition_to_draft(condition)?,
            draft_from_expr(inner)?,
        ),
    };
    Ok(normalize_draft(node))
}

fn and_rules(mut items: Vec<RuleExpr>) -> RuleExpr {
    if items.len() == 1 { items.remove(0) } else { RuleExpr::And(items) }
}

/// 组内按成员顺序拼接规则：AND 优先，OR 分段。
fn combine_rules(items: Vec<(RuleExpr, LogicOp)>) -> Option<RuleExpr> {
    if items.is_empty() { return None; }
    let mut terms = Vec::new();
    let mut and_items = Vec::new();
    for (expr, conn) in items {
        and_items.push(expr);
        if conn == LogicOp::Or {
            terms.push(and_rules(std::mem::take(&mut and_items)));
        }
    }
    if !and_items.is_empty() { terms.push(and_rules(and_items)); }
    match terms.len() {
        0 => None,
        1 => Some(terms.remove(0)),
        _ => Some(RuleExpr::Or(terms)),
    }
}

fn expr_from_draft(node: &DraftExpr) -> Option<RuleExpr> {
    match node {
        DraftExpr::Validator(v) => Some(RuleExpr::Validator(v.clone())),
        DraftExpr::Condition(_) => None,
        DraftExpr::Group { children, conns } => {
            let mut conds = Vec::new();
            let mut rules = Vec::new();
            for (i, child) in children.iter().enumerate() {
                let conn = conn_at(conns, i);
                if is_condition_only(child) {
                    conds.push((condition_of(child)?, conn));
                } else {
                    rules.push((expr_from_draft(child)?, conn));
                }
            }
            let rule_expr = combine_rules(rules)?;
            if conds.is_empty() {
                Some(rule_expr)
            } else {
                Some(RuleExpr::When {
                    condition: combine_conditions(conds)?,
                    inner: Box::new(rule_expr),
                })
            }
        }
    }
}

fn draft_node<'a>(root: &'a DraftExpr, path: &[usize]) -> Option<&'a DraftExpr> {
    let mut cur = root;
    for &i in path {
        cur = match cur {
            DraftExpr::Group { children, .. } => children.get(i)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn draft_node_mut<'a>(root: &'a mut DraftExpr, path: &[usize]) -> Option<&'a mut DraftExpr> {
    let mut cur = root;
    for &i in path {
        cur = match cur {
            DraftExpr::Group { children, .. } => children.get_mut(i)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn group_mut<'a>(root: &'a mut DraftExpr, path: &[usize]) -> Option<&'a mut DraftExpr> {
    let node = draft_node_mut(root, path)?;
    match node {
        DraftExpr::Group { .. } => Some(node),
        _ => None,
    }
}

fn remove_node(root: &mut DraftExpr, path: &[usize]) -> Option<DraftExpr> {
    let (last, parent) = path.split_last()?;
    let node = group_mut(root, parent)?;
    let DraftExpr::Group { children, conns } = node else { return None };
    if *last < children.len() {
        if *last < conns.len() { conns.remove(*last); }
        Some(children.remove(*last))
    } else {
        None
    }
}

fn swap_node(root: &mut DraftExpr, path: &[usize], up: bool) -> bool {
    let Some((last, parent)) = path.split_last() else { return false };
    let Some(node) = group_mut(root, parent) else { return false };
    let DraftExpr::Group { children, conns } = node else { return false };
    let j = *last;
    let k = if up {
        j.checked_sub(1)
    } else {
        j.checked_add(1).filter(|k| *k < children.len())
    };
    if let Some(k) = k {
        children.swap(j, k);
        if j < conns.len() && k < conns.len() { conns.swap(j, k); }
        true
    } else {
        false
    }
}

fn set_connector_at(root: &mut DraftExpr, path: &[usize], op: LogicOp) -> bool {
    let Some((last, parent)) = path.split_last() else { return false };
    let Some(node) = draft_node_mut(root, parent) else { return false };
    let DraftExpr::Group { conns, .. } = node else { return false };
    if let Some(slot) = conns.get_mut(*last) {
        *slot = op;
        true
    } else {
        false
    }
}

fn push_child(root: &mut DraftExpr, path: &[usize], child: DraftExpr) -> bool {
    let Some(node) = group_mut(root, path) else { return false };
    let DraftExpr::Group { children, conns } = node else { return false };
    children.push(child);
    conns.push(LogicOp::And);
    true
}

/// 在 path 指向的成员之后插入一个同级成员；path 为空时插入根层开头。
pub(crate) fn insert_sibling(
    root: &mut DraftExpr,
    path: &[usize],
    child: DraftExpr,
) -> Option<Vec<usize>> {
    let (parent_path, insert_at) = if path.is_empty() {
        (&[][..], 0usize)
    } else {
        let (last, parent) = path.split_last()?;
        (parent, *last + 1)
    };
    let node = group_mut(root, parent_path)?;
    let DraftExpr::Group { children, conns } = node else { return None };
    let idx = insert_at.min(children.len());
    children.insert(idx, child);
    conns.insert(idx, LogicOp::And);
    if idx > 0 { conns[idx - 1] = LogicOp::And; }
    let mut new_path = parent_path.to_vec();
    new_path.push(idx);
    Some(new_path)
}

/// 根层「合并」：把根层全部成员包进一个组。
pub(crate) fn merge_root(root: &mut DraftExpr) -> bool {
    let DraftExpr::Group { children, conns } = root else { return false };
    if children.len() < 2 { return false; }
    let old_children = std::mem::replace(children, Vec::new());
    let old_conns = std::mem::replace(conns, Vec::new());
    children.push(group_with(old_children, old_conns));
    conns.push(LogicOp::And);
    true
}

/// 组尾「分散」：取消这个组，把成员原位展开到上一层。
pub(crate) fn dissolve_group(root: &mut DraftExpr, path: &[usize]) -> Option<Vec<usize>> {
    let (last, parent_path) = path.split_last()?;
    let parent = group_mut(root, parent_path)?;
    let DraftExpr::Group { children, conns } = parent else { return None };
    let idx = *last;
    if idx >= children.len() { return None; }
    if !matches!(children[idx], DraftExpr::Group { .. }) { return None; }
    let node = children.remove(idx);
    let external = if idx < conns.len() { conns.remove(idx) } else { LogicOp::And };
    let DraftExpr::Group { children: moved, conns: mut moved_conns } = node else {
        unreachable!("checked group above")
    };
    if moved.is_empty() {
        return Some(parent_path.to_vec());
    }
    if let Some(last_conn) = moved_conns.last_mut() { *last_conn = external; }
    for (offset, child) in moved.into_iter().enumerate() {
        children.insert(idx + offset, child);
    }
    for (offset, conn) in moved_conns.into_iter().enumerate() {
        conns.insert(idx + offset, conn);
    }
    let mut new_path = parent_path.to_vec();
    new_path.push(idx);
    Some(new_path)
}

fn group_with_one(child: DraftExpr) -> DraftExpr {
    group_with(vec![child], vec![LogicOp::And])
}

/// 删除后清理空组：组内成员全删光时，整个组从父层移除。
pub(crate) fn prune_empty_groups(node: &mut DraftExpr) {
    let DraftExpr::Group { children, conns } = node else { return };
    let mut i = 0;
    while i < children.len() {
        if matches!(children[i], DraftExpr::Group { .. }) {
            prune_empty_groups(&mut children[i]);
            if matches!(&children[i], DraftExpr::Group { children, .. } if children.is_empty()) {
                children.remove(i);
                if i < conns.len() { conns.remove(i); }
                continue;
            }
        }
        i += 1;
    }
}

// ── 面板状态 ────────────────────────────────────────────────────────────

/// 有未保存改动时，用户请求切换到的目标；确认丢弃后再真正加载。
#[derive(Clone, Debug)]
pub(crate) enum PendingSwitch {
    Tab(RuleScope),
    Column(String),
}

/// 一条自定义规则的编辑稿。规则之间互相独立，各自有级别。
pub struct RuleDraft {
    /// 对应 `original` 里的下标；`None` = 本次新建、尚未保存。
    pub original: Option<usize>,
    pub draft: DraftExpr,
    pub level: Severity,
    pub collapsed: bool,
    pub dirty: bool,
}

impl RuleDraft {
    fn new(draft: DraftExpr, level: Severity, original: Option<usize>) -> Self {
        Self { original, draft, level, collapsed: false, dirty: false }
    }
}

pub struct RulesPanelState {
    pub open: bool,
    pub group: String,
    pub name: String,
    pub original: Vec<Rule>,
    /// 打开面板时的快照；取消时恢复到这里，而不是恢复到中途保存过的版本。
    pub session_original: Vec<Rule>,
    pub history_undo_len: usize,
    pub tab: RuleScope,
    pub column: Option<String>,
    /// 所有自定义规则，按展示顺序排列；列表顺序 = 保存顺序。
    pub drafts: Vec<RuleDraft>,
    /// 无法图形化、但仍需原样保留的 `original` 下标。
    pub unparsed: Vec<usize>,
    /// 当前激活卡片；-1 = 没有规则。
    pub selected_rule: i32,
    pub mode: EditorMode,
    pub quick_function: String,
    pub selected: Option<RowKey>,
    pub row_keys: Vec<RowKey>,
    pub row_kinds: Vec<String>,
    pub dirty: bool,
    pub palette_open: bool,
    pub palette_target: Option<RowKey>,
    pub palette_replace: Option<RowKey>,
    pub palette_selected: Option<String>,
    pub palette_search: String,
    pub palette_filter: i32,
    /// 未保存改动时请求切换的目标；确认后丢弃当前草稿再切换。
    pub pending_switch: Option<PendingSwitch>,
    pub discard_open: bool,
    pub discard_message: String,
}

impl RulesPanelState {
    pub fn new() -> Self {
        Self {
            open: false,
            group: String::new(),
            name: String::new(),
            original: Vec::new(),
            session_original: Vec::new(),
            history_undo_len: 0,
            tab: RuleScope::Column,
            column: None,
            drafts: Vec::new(),
            unparsed: Vec::new(),
            selected_rule: -1,
            mode: EditorMode::Advanced,
            quick_function: "unique".to_string(),
            selected: None,
            row_keys: Vec::new(),
            row_kinds: Vec::new(),
            dirty: false,
            palette_open: false,
            palette_target: None,
            palette_replace: None,
            palette_selected: None,
            palette_search: String::new(),
            palette_filter: 0,
            pending_switch: None,
            discard_open: false,
            discard_message: String::new(),
        }
    }

    pub(crate) fn active_draft_mut(&mut self) -> Option<&mut DraftExpr> {
        let i = self.selected_rule;
        if i < 0 { return None; }
        self.drafts.get_mut(i as usize).map(|d| &mut d.draft)
    }

    pub(crate) fn mark_dirty(&mut self) {
        self.dirty = true;
        let i = self.selected_rule;
        if i >= 0 {
            if let Some(d) = self.drafts.get_mut(i as usize) { d.dirty = true; }
        }
    }
}

impl Default for RulesPanelState {
    fn default() -> Self { Self::new() }
}

fn node_mode(st: &AppState, group: &str, name: &str) -> Option<NodeMode> {
    st.engine.node_rules(group, name).map(|(m, _)| m)
}

fn node_rules(st: &AppState, group: &str, name: &str) -> Vec<Rule> {
    st.engine.node_rules(group, name).map(|(_, r)| r).unwrap_or_default()
}

fn node_issues(st: &AppState, group: &str, name: &str) -> Vec<ValidationError> {
    let project = st.engine.project();
    let sep = &project.schema.separators;
    let refs = RefIndex::build(&project.groups);
    let cidx = RuleIndex::build(&project.groups);
    if let Some(t) = st.engine.find_table(group, name) {
        return validate_table(t, sep, Some(&refs), Some(&cidx));
    }
    if let Some(e) = st.engine.find_enum(group, name) {
        return validate_enum(e, Some(&cidx));
    }
    Vec::new()
}

fn current_target(st: &AppState) -> Option<RuleTarget> {
    match st.rules_panel.tab {
        RuleScope::Column => st.rules_panel.column.clone().map(RuleTarget::Column),
        RuleScope::Table => Some(RuleTarget::Table),
    }
}

fn current_anchor(st: &AppState) -> Option<String> {
    if st.rules_panel.tab == RuleScope::Column {
        st.rules_panel.column.clone()
    } else {
        None
    }
}

fn normalize_selection(st: &mut AppState) {
    let group = st.rules_panel.group.clone();
    let name = st.rules_panel.name.clone();
    let mode = node_mode(st, &group, &name);
    if st.rules_panel.tab == RuleScope::Table && mode != Some(NodeMode::Table) {
        st.rules_panel.tab = RuleScope::Column;
    }
    let specs = st.engine.node_column_classes(&group, &name);
    let ok = st.rules_panel.column.as_ref()
        .map_or(false, |c| specs.iter().any(|(n, _)| n == c));
    if !ok {
        st.rules_panel.column = specs.iter()
            .find(|(_, c)| c.is_some())
            .or_else(|| specs.first())
            .map(|(n, _)| n.clone());
    }
    if st.rules_panel.column.is_none() && mode == Some(NodeMode::Table) {
        st.rules_panel.tab = RuleScope::Table;
    }
}

fn default_validator(
    key: &str,
    scope: RuleScope,
    primary: Option<&str>,
    specs: &[ColumnSpec],
    st: &AppState,
) -> RuleKind {
    let columns = scalar_columns(specs);
    let col = if scope == RuleScope::Column {
        primary
            .map(str::to_string)
            .filter(|s| !s.is_empty())
            .or_else(|| columns.first().cloned())
            .unwrap_or_default()
    } else {
        primary
            .filter(|p| validator_applies(key, column_class(specs, p)))
            .map(str::to_string)
            .or_else(|| columns.iter()
                .find(|c| validator_applies(key, column_class(specs, c)))
                .cloned())
            .or_else(|| primary.map(str::to_string))
            .or_else(|| columns.first().cloned())
            .unwrap_or_default()
    };
    let other = columns.iter()
        .find(|c| **c != col && validator_applies("compare", column_class(specs, c)))
        .cloned()
        .unwrap_or_default();
    match key {
        "unique" => RuleKind::Unique { column: col },
        "non_empty" => RuleKind::NonEmpty { column: col },
        "positive_int" => RuleKind::PositiveInt { column: col },
        "upper_snake" => RuleKind::UpperSnake { column: col },
        "range" => RuleKind::Range { column: col, min: "0".into(), max: String::new() },
        "len" => RuleKind::Len { column: col, min: "1".into(), max: String::new() },
        "one_of" => RuleKind::OneOf { column: col, values: Vec::new() },
        "matches" => RuleKind::Matches { column: col, pattern: String::new() },
        "ref_exists" => {
            let targets = target_nodes(st);
            let node = targets.first().cloned().unwrap_or_default();
            let target_col = target_columns(st, &node).into_iter().next().unwrap_or_default();
            RuleKind::RefExists { column: col, target_node: node, target_column: target_col }
        }
        "compare" => RuleKind::Compare { column: col, op: CompareOp::Le, other },
        "all_non_empty" => RuleKind::AllNonEmpty { columns: columns.into_iter().take(2).collect() },
        "at_least_one" => RuleKind::AtLeastOne { columns: columns.into_iter().take(2).collect() },
        "exactly_one" => RuleKind::ExactlyOne { columns: columns.into_iter().take(2).collect() },
        "row_count" => RuleKind::RowCount { min: "1".into(), max: String::new() },
        "id_contiguous" => RuleKind::IdContiguous,
        "unique_combination" =>
            RuleKind::UniqueCombination { columns: columns.into_iter().take(2).collect() },
        _ => {
            let _ = scope;
            RuleKind::NonEmpty { column: col }
        }
    }
}

fn set_primary_column(kind: &mut RuleKind, name: &str) {
    match kind {
        RuleKind::Unique { column }
        | RuleKind::NonEmpty { column }
        | RuleKind::Range { column, .. }
        | RuleKind::Len { column, .. }
        | RuleKind::OneOf { column, .. }
        | RuleKind::Matches { column, .. }
        | RuleKind::RefExists { column, .. }
        | RuleKind::Compare { column, .. }
        | RuleKind::PositiveInt { column }
        | RuleKind::UpperSnake { column } => *column = name.to_string(),
        _ => {}
    }
}

fn new_default_node(
    st: &AppState,
    specs: &[ColumnSpec],
    primary: Option<&str>,
) -> DraftExpr {
    let scope = st.rules_panel.tab;
    let class = if scope == RuleScope::Column {
        st.rules_panel.column.as_deref().and_then(|c| column_class(specs, c))
    } else {
        None
    };
    let key = validator_keys(scope).into_iter()
        .find(|k| scope == RuleScope::Table || validator_applies(k, class))
        .unwrap_or("unique");
    DraftExpr::Validator(default_validator(key, scope, primary, specs, st))
}

fn new_default_group(
    st: &AppState,
    specs: &[ColumnSpec],
    primary: Option<&str>,
) -> DraftExpr {
    group_with_one(new_default_node(st, specs, primary))
}

fn selected_condition<'a>(st: &'a RulesPanelState) -> Option<&'a LeafCond> {
    let key = st.selected.as_ref()?;
    let draft = &st.drafts.get(key.rule)?.draft;
    match draft_node(draft, &key.path)? {
        DraftExpr::Condition(l) => Some(l),
        _ => None,
    }
}

fn selected_condition_mut(st: &mut RulesPanelState) -> Option<&mut LeafCond> {
    let key = st.selected.clone()?;
    let draft = &mut st.drafts.get_mut(key.rule)?.draft;
    match draft_node_mut(draft, &key.path)? {
        DraftExpr::Condition(l) => Some(l),
        _ => None,
    }
}

fn selected_validator<'a>(st: &'a RulesPanelState) -> Option<&'a RuleKind> {
    let key = st.selected.as_ref()?;
    let draft = &st.drafts.get(key.rule)?.draft;
    match draft_node(draft, &key.path)? {
        DraftExpr::Validator(v) => Some(v),
        _ => None,
    }
}

fn selected_validator_mut<'a>(st: &'a mut RulesPanelState) -> Option<&'a mut RuleKind> {
    let key = st.selected.clone()?;
    let draft = &mut st.drafts.get_mut(key.rule)?.draft;
    match draft_node_mut(draft, &key.path)? {
        DraftExpr::Validator(v) => Some(v),
        _ => None,
    }
}

fn condition_leaf_mut<'a>(
    st: &'a mut RulesPanelState,
    row: usize,
) -> Option<&'a mut LeafCond> {
    let key = st.row_keys.get(row)?.clone();
    st.selected_rule = key.rule as i32;
    let draft = &mut st.drafts.get_mut(key.rule)?.draft;
    match draft_node_mut(draft, &key.path)? {
        DraftExpr::Condition(l) => Some(l),
        _ => None,
    }
}

fn class_label(class: Option<ValueClass>) -> &'static str {
    match class {
        Some(ValueClass::Int) => "整数",
        Some(ValueClass::Float) => "浮点",
        Some(ValueClass::Text) => "文本",
        None => "不支持规则",
    }
}

fn level_index(level: Severity) -> i32 {
    match level { Severity::Error => 0, Severity::Warning => 1, Severity::Info => 2 }
}

fn index_of(list: &[String], value: &str) -> i32 {
    list.iter().position(|x| x == value).map(|i| i as i32).unwrap_or(0)
}

fn split_values(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' => {
                if in_quotes && chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quotes = !in_quotes;
                }
            }
            ',' | '，' if !in_quotes => {
                let value = cur.trim().to_string();
                if !value.is_empty() { out.push(value); }
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    let value = cur.trim().to_string();
    if !value.is_empty() { out.push(value); }
    out
}

fn join_values(values: &[String]) -> String {
    values.iter().map(|v| {
        if v.contains(',') || v.contains('，') || v.contains('"') || v.trim() != v {
            format!("\"{}\"", v.replace('"', "\"\""))
        } else {
            v.clone()
        }
    }).collect::<Vec<_>>().join(", ")
}

fn target_nodes(st: &AppState) -> Vec<String> {
    st.engine.rule_target_nodes().into_iter()
        .filter(|n| st.engine.node_column_classes_by_name(n).iter().any(|(_, c)| c.is_some()))
        .collect()
}

fn target_columns(st: &AppState, node: &str) -> Vec<String> {
    st.engine.node_column_classes_by_name(node).into_iter()
        .filter(|(_, c)| c.is_some())
        .map(|(n, _)| n)
        .collect()
}

fn commit_rules(state: &Rc<RefCell<AppState>>, rules: Vec<Rule>) -> bool {
    let mut st = state.borrow_mut();
    let group = st.rules_panel.group.clone();
    let name = st.rules_panel.name.clone();
    let ok = st.engine.set_node_rules(&group, &name, rules);
    if ok && st.realtime_validate {
        st.engine.revalidate(&group, &name);
    }
    ok
}

// ── 构建行 ──────────────────────────────────────────────────────────────

fn empty_strings() -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(Vec::<SharedString>::new()))
}

fn base_row(depth: i32, kind: &str, title: String, detail: String) -> RuleTreeRow {
    RuleTreeRow {
        depth,
        kind: SharedString::from(kind),
        title: SharedString::from(title),
        detail: SharedString::from(detail),
        rule_index: -1,
        rule_level_index: 0,
        rule_summary: SharedString::from(""),
        rule_collapsed: false,
        rule_active: false,
        connector: SharedString::from(""),
        connector_index: 0,
        selected: false,
        can_delete: false,
        can_move_up: false,
        can_move_down: false,
        logic_index: 0,
        not: false,
        can_add_validator: false,
        can_add_group: false,
        can_add_when: false,
        can_add_condition: false,
        can_add_condition_group: false,
        has_child_groups: false,
        can_dissolve: false,
        can_merge: false,
        column_index: 0,
        op_index: 0,
        op_options: empty_strings(),
        value_mode: SharedString::from("single"),
        value: SharedString::from(""),
        pattern: SharedString::from(""),
        values: empty_strings(),
    }
}

fn condition_row_data(
    depth: i32,
    l: &LeafCond,
    connector: &str,
    specs: &[ColumnSpec],
) -> RuleTreeRow {
    let columns = scalar_columns(specs);
    let op_options = leaf_op_options(column_class(specs, &l.column));
    let op_index = op_options.iter().position(|x| x == l.op.label()).unwrap_or(0) as i32;
    let detail = match l.op {
        LeafOp::In => format!(" in ({})", l.values.join(", ")),
        LeafOp::Match => format!(" match \"{}\"", l.pattern),
        _ => format!(" {} {}", l.op.label(), l.value),
    };
    let mut row = base_row(depth, "cond_leaf", l.column.clone(), detail);
    row.connector = SharedString::from(connector);
    row.connector_index = if connector == "or" { 1 } else { 0 };
    row.logic_index = if connector == "or" { 1 } else { 0 };
    row.column_index = index_of(&columns, &l.column);
    row.op_index = op_index;
    row.op_options = ModelRc::new(VecModel::from(
        op_options.into_iter().map(SharedString::from).collect::<Vec<_>>(),
    ));
    row.value_mode = SharedString::from(l.op.value_mode());
    row.value = SharedString::from(l.value.clone());
    row.pattern = SharedString::from(l.pattern.clone());
    row.values = ModelRc::new(VecModel::from(
        l.values.iter().map(|v| SharedString::from(v.clone())).collect::<Vec<_>>(),
    ));
    row
}

fn conn_label(conn: LogicOp) -> &'static str {
    if conn == LogicOp::Or { "or" } else { "and" }
}

fn set_connector(row: &mut RuleTreeRow, conn: LogicOp) {
    row.connector = SharedString::from(conn_label(conn));
    row.connector_index = conn.index();
    row.logic_index = conn.index();
}

fn flatten_rows(
    node: &DraftExpr,
    path: &mut Vec<usize>,
    depth: i32,
    connector: Option<LogicOp>,
    parent_is_root: bool,
    is_last: bool,
    specs: &[ColumnSpec],
    scope: RuleScope,
    rule: usize,
    rows: &mut Vec<TreeRowData>,
) {
    match node {
        DraftExpr::Group { children, conns } => {
            if path.is_empty() {
                // 根组不画标题；只有能「合并」时才需要一行放按钮。
                if children.len() >= 2 {
                    let mut head = base_row(0, "root_head", String::new(), String::new());
                    head.can_merge = true;
                    rows.push(TreeRowData { row: head, key: RowKey { rule, path: Vec::new() } });
                }
                if children.is_empty() {
                    let mut empty = base_row(0, "root_empty", String::new(), String::new());
                    empty.can_add_validator = true;
                    empty.can_add_group = true;
                    rows.push(TreeRowData {
                        row: empty,
                        key: RowKey { rule, path: Vec::new() },
                    });
                } else {
                    let count = children.len();
                    for (i, child) in children.iter().enumerate() {
                        path.push(i);
                        flatten_rows(
                            child, path, 0, Some(conn_at(conns, i)), true, i + 1 == count,
                            specs, scope, rule, rows,
                        );
                        path.pop();
                    }
                }
                return;
            }

            let open = base_row(depth, "group_open", "组".into(), String::new());
            rows.push(TreeRowData { row: open, key: RowKey { rule, path: path.clone() } });

            let count = children.len();
            for (i, child) in children.iter().enumerate() {
                path.push(i);
                flatten_rows(
                    child, path, depth + 1, Some(conn_at(conns, i)), false, i + 1 == count,
                    specs, scope, rule, rows,
                );
                path.pop();
            }

            let mut close = base_row(depth, "group_close", String::new(), String::new());
            close.can_add_validator = true;
            close.can_add_group = true;
            close.can_delete = true;
            close.can_dissolve = true;
            close.has_child_groups = children.iter().any(|c| matches!(c, DraftExpr::Group { .. }));
            if !is_last {
                if let Some(c) = connector { set_connector(&mut close, c); }
            }
            rows.push(TreeRowData { row: close, key: RowKey { rule, path: path.clone() } });
        }
        DraftExpr::Condition(l) => {
            let mut row = condition_row_data(depth, l, "", specs);
            row.can_delete = true;
            row.can_add_validator = true;
            row.can_add_group = true;
            if !is_last || parent_is_root {
                if let Some(c) = connector { set_connector(&mut row, c); }
            }
            rows.push(TreeRowData { row, key: RowKey { rule, path: path.clone() } });
        }
        DraftExpr::Validator(v) => {
            let full = v.expr_string(scope == RuleScope::Column);
            let detail = full.strip_prefix(v.name()).unwrap_or(&full).to_string();
            let mut row = base_row(depth, "validator", v.name().to_string(), detail);
            row.can_delete = true;
            row.can_add_validator = true;
            row.can_add_group = true;
            if !is_last || parent_is_root {
                if let Some(c) = connector { set_connector(&mut row, c); }
            }
            rows.push(TreeRowData { row, key: RowKey { rule, path: path.clone() } });
        }
    }
}

fn draft_summary(draft: &DraftExpr, target: Option<&RuleTarget>) -> String {
    let expr = match expr_from_draft(draft) {
        Some(e) => e,
        None => return "（规则不完整）".to_string(),
    };
    match target {
        Some(RuleTarget::Column(c)) => format!("column {}: {}", c, expr.expr_string(true)),
        _ => format!("table: {}", expr.expr_string(false)),
    }
}

fn mark_selected(st: &AppState, rows: &mut [TreeRowData]) {
    for r in rows {
        let selectable = matches!(r.row.kind.as_str(), "validator" | "cond_leaf");
        r.row.selected = selectable && st.rules_panel.selected.as_ref() == Some(&r.key);
    }
}

fn build_rows(st: &AppState, specs: &[ColumnSpec]) -> Vec<TreeRowData> {
    let target = current_target(st);
    let mut rows = Vec::new();
    for (ri, rd) in st.rules_panel.drafts.iter().enumerate() {
        let summary = draft_summary(&rd.draft, target.as_ref());
        let root_len = match &rd.draft {
            DraftExpr::Group { children, .. } => children.len(),
            _ => 0,
        };
        let mut head = base_row(0, "rule_head", format!("规则 {}", ri + 1), String::new());
        head.rule_index = ri as i32;
        head.rule_level_index = level_index(rd.level);
        head.rule_summary = SharedString::from(summary.clone());
        head.rule_collapsed = rd.collapsed;
        head.rule_active = st.rules_panel.selected_rule == ri as i32;
        head.can_delete = true;
        head.can_merge = root_len >= 2;
        rows.push(TreeRowData {
            row: head,
            key: RowKey { rule: ri, path: Vec::new() },
        });

        if !rd.collapsed {
            let start = rows.len();
            let mut path = Vec::new();
            flatten_rows(
                &rd.draft, &mut path, 0, None, true, true,
                specs, st.rules_panel.tab, ri, &mut rows,
            );
            for r in &mut rows[start..] {
                r.row.rule_index = ri as i32;
                r.row.rule_active = st.rules_panel.selected_rule == ri as i32;
            }
            let mut expr = base_row(0, "expr", "表达式".into(), summary);
            expr.rule_index = ri as i32;
            expr.rule_active = st.rules_panel.selected_rule == ri as i32;
            rows.push(TreeRowData {
                row: expr,
                key: RowKey { rule: ri, path: Vec::new() },
            });
        }

        let mut gap = base_row(0, "gap", String::new(), String::new());
        gap.rule_index = ri as i32;
        rows.push(TreeRowData {
            row: gap,
            key: RowKey { rule: ri, path: Vec::new() },
        });
    }
    mark_selected(st, &mut rows);
    rows
}

fn first_validator_path(node: &DraftExpr, path: &mut Vec<usize>) -> Option<Vec<usize>> {
    match node {
        DraftExpr::Validator(_) => Some(path.clone()),
        DraftExpr::Group { children, .. } => {
            for (i, child) in children.iter().enumerate() {
                path.push(i);
                if let Some(p) = first_validator_path(child, path) {
                    return Some(p);
                }
                path.pop();
            }
            None
        }
        DraftExpr::Condition(_) => None,
    }
}

fn build_rule_draft(st: &AppState, rd: &RuleDraft) -> Option<Rule> {
    let target = current_target(st)?;
    let expr = expr_from_draft(&rd.draft)?;
    Some(Rule {
        target,
        expr,
        level: rd.level,
        source: RuleSource::User,
    })
}

fn build_rules(st: &AppState) -> Option<Vec<Rule>> {
    st.rules_panel.drafts.iter().map(|d| build_rule_draft(st, d)).collect()
}

fn form_status(st: &AppState) -> (bool, String) {
    let group = st.rules_panel.group.clone();
    let name = st.rules_panel.name.clone();
    let specs = st.engine.node_column_classes(&group, &name);
    let mode = match node_mode(st, &group, &name) {
        Some(m) => m,
        None => return (false, String::new()),
    };
    match st.rules_panel.tab {
        RuleScope::Column => {
            let Some(col) = st.rules_panel.column.as_deref() else {
                return (false, "请选择一个列".to_string());
            };
            if column_class(&specs, col).is_none() {
                return (false, DESC_UNSUPPORTED.to_string());
            }
        }
        RuleScope::Table => {
            if mode != NodeMode::Table {
                return (false, "表级规则只能用于 Table 节点".to_string());
            }
        }
    }
    let project = st.engine.project();
    let cidx = RuleIndex::build(&project.groups);
    let mut built: Vec<(usize, Rule)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    for (i, rd) in st.rules_panel.drafts.iter().enumerate() {
        let Some(rule) = build_rule_draft(st, rd) else {
            return (false, format!("规则 {}：{}", i + 1, HINT_INCOMPLETE));
        };
        if !rule_params_valid(&rule.expr) {
            return (false, format!(
                "规则 {}：参数不完整或不合法（区间两端不能都空、正则不能为空、列不能为空）。",
                i + 1
            ));
        }
        let errs = validate_rules_schema(mode, &name, &specs, &[rule.clone()], &cidx);
        if let Some(e) = errs.iter().find(|e| e.severity == Severity::Error) {
            return (false, format!("规则 {}：{}", i + 1, e.message));
        }
        // 冗余规则 / 内置冲突是警告级：允许保存，只在状态条提示。
        for e in errs.iter() {
            warnings.push(format!("规则 {}：{}", i + 1, e.message));
        }
        if built.iter().any(|(_, r)| r.target == rule.target && r.expr == rule.expr) {
            return (false, format!("规则 {}：{}", i + 1, HINT_DUPLICATE));
        }
        built.push((i, rule));
    }
    (true, warnings.join("；"))
}

/// 按当前作用域把草稿写回节点规则；其它作用域与无法图形化的旧规则原样保留。
fn commit_drafts(state: &Rc<RefCell<AppState>>) -> bool {
    let merged = {
        let st = state.borrow();
        let Some(target) = current_target(&st) else { return false };
        let Some(new_rules) = build_rules(&st) else { return false };
        let mut mapped = std::collections::HashMap::new();
        for (d, rule) in st.rules_panel.drafts.iter().zip(new_rules.into_iter()) {
            if let Some(orig) = d.original { mapped.insert(orig, rule); }
        }
        let mut merged = Vec::new();
        for (i, r) in st.rules_panel.original.iter().enumerate() {
            if r.target != target {
                merged.push(r.clone());
            } else if let Some(replacement) = mapped.remove(&i) {
                merged.push(replacement);
            } else if st.rules_panel.unparsed.contains(&i) {
                // 图形编辑器无法表达的旧规则，原样保留不丢。
                merged.push(r.clone());
            }
            // 其余目标规则：草稿里已删除，保存时一并删除。
        }
        for d in st.rules_panel.drafts.iter() {
            if d.original.is_none() {
                if let Some(rule) = build_rule_draft(&st, d) { merged.push(rule); }
            }
        }
        merged
    };
    if !commit_rules(state, merged) { return false; }
    {
        let mut st = state.borrow_mut();
        reload_drafts(&mut st);
        st.rules_panel.dirty = false;
    }
    true
}

/// 从节点的已保存规则重建当前作用域的草稿列表。保留尽可能接近的选中卡片。
fn reload_drafts(st: &mut AppState) {
    let group = st.rules_panel.group.clone();
    let name = st.rules_panel.name.clone();
    let target = current_target(st);
    let rules = node_rules(st, &group, &name);
    let keep = st.rules_panel.selected_rule;
    st.rules_panel.original = rules.clone();
    st.rules_panel.drafts.clear();
    st.rules_panel.unparsed.clear();
    if let Some(target) = target {
        for (i, r) in rules.iter().enumerate() {
            if r.target != target { continue; }
            match draft_from_expr(&r.expr) {
                Ok(draft) => st.rules_panel.drafts.push(RuleDraft::new(draft, r.level, Some(i))),
                Err(e) => {
                    st.rules_panel.unparsed.push(i);
                    st.engine.ui_log(format!("规则 #{} 暂不支持图形编辑：{}", i + 1, e));
                }
            }
        }
    }
    let len = st.rules_panel.drafts.len() as i32;
    st.rules_panel.selected_rule = if len == 0 { -1 } else { keep.clamp(0, len - 1) };
    st.rules_panel.selected = None;
    if st.rules_panel.selected_rule >= 0 {
        let i = st.rules_panel.selected_rule as usize;
        if let Some(path) = first_validator_path(&st.rules_panel.drafts[i].draft, &mut Vec::new()) {
            st.rules_panel.selected_rule = i as i32;
            st.rules_panel.selected = Some(RowKey { rule: i, path });
        }
    }
    st.rules_panel.dirty = false;
    st.rules_panel.palette_open = false;
    st.rules_panel.palette_target = None;
    st.rules_panel.palette_replace = None;
}

fn palette_first(scope: RuleScope) -> Option<String> {
    validator_keys(scope).first().map(|s| s.to_string())
}

fn palette_item_matches(key: &str, search: &str) -> bool {
    if search.trim().is_empty() { return true; }
    let q = search.trim().to_lowercase();
    key.to_lowercase().contains(&q)
        || validator_label(key).to_lowercase().contains(&q)
        || validator_desc(key).to_lowercase().contains(&q)
}

fn palette_items(st: &AppState) -> Vec<RuleFunctionItem> {
    let scope = st.rules_panel.tab;
    let filter = st.rules_panel.palette_filter;
    let search = st.rules_panel.palette_search.as_str();
    let selected = st.rules_panel.palette_selected.as_deref();
    let mut out = Vec::new();

    let push_group = |title: &str, keys: &[&str], gi: i32, out: &mut Vec<RuleFunctionItem>| {
        if filter != 0 && filter != gi { return; }
        let mut items = Vec::new();
        for key in keys {
            if !palette_item_matches(key, search) { continue; }
            items.push(RuleFunctionItem {
                is_group: false,
                group: SharedString::from(title),
                key: SharedString::from(*key),
                label: SharedString::from(validator_label(key)),
                summary: SharedString::from(validator_desc(key)),
                tags: SharedString::from(validator_tags(key)),
                enabled: true,
                selected: selected == Some(*key),
            });
        }
        if !items.is_empty() {
            out.push(RuleFunctionItem {
                is_group: true,
                group: SharedString::from(title),
                key: SharedString::from(""),
                label: SharedString::from(""),
                summary: SharedString::from(""),
                tags: SharedString::from(""),
                enabled: false,
                selected: false,
            });
            out.extend(items);
        }
    };

    let specs = st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name);
    let class = if scope == RuleScope::Column {
        st.rules_panel.column.as_deref().and_then(|c| column_class(&specs, c))
    } else {
        None
    };
    let column_keys: Vec<&str> = COLUMN_VALIDATORS.iter().copied()
        .filter(|k| scope == RuleScope::Table || validator_applies(k, class))
        .collect();
    push_group("列校验", &column_keys, 1, &mut out);
    if scope == RuleScope::Table {
        push_group("行级校验", ROW_VALIDATORS, 2, &mut out);
        push_group("全表校验", TABLE_VALIDATORS, 3, &mut out);
    }
    out
}

// ── 打开 / 推送 ─────────────────────────────────────────────────────────

pub fn open_for_selected(state: &Rc<RefCell<AppState>>) {
    let target = {
        let s = state.borrow();
        match &s.selected {
            Some(SelectedNode::Table { group, name, .. })
            | Some(SelectedNode::Enum { group, name, .. }) => Some((group.clone(), name.clone())),
            _ => None,
        }
    };
    if let Some((group, name)) = target {
        open_for_node(state, &group, &name);
    }
}

pub fn open_for_node(state: &Rc<RefCell<AppState>>, group: &str, name: &str) {
    let mut st = state.borrow_mut();
    let Some((mode, rules)) = st.engine.node_rules(group, name) else { return };
    let specs = st.engine.node_column_classes(group, name);
    let column = rules.iter().find_map(|r| match &r.target {
        RuleTarget::Column(c) => Some(c.clone()),
        RuleTarget::Table => None,
    })
        .or_else(|| specs.iter().find(|(_, c)| c.is_some()).map(|(n, _)| n.clone()))
        .or_else(|| specs.first().map(|(n, _)| n.clone()));
    let tab = if column.is_none() && mode == NodeMode::Table {
        RuleScope::Table
    } else {
        RuleScope::Column
    };
    let undo_len = st.engine.active_project_id()
        .map(|pid| st.engine.history_len(pid).0)
        .unwrap_or(0);

    st.rules_panel.open = true;
    st.rules_panel.group = group.to_string();
    st.rules_panel.name = name.to_string();
    st.rules_panel.history_undo_len = undo_len;
    st.rules_panel.session_original = rules;
    st.rules_panel.tab = tab;
    st.rules_panel.column = column;
    st.rules_panel.mode = EditorMode::Advanced;
    st.rules_panel.selected_rule = 0;
    st.rules_panel.row_keys.clear();
    st.rules_panel.row_kinds.clear();
    st.rules_panel.palette_selected = None;
    st.rules_panel.palette_search.clear();
    st.rules_panel.palette_filter = 0;
    st.rules_panel.pending_switch = None;
    st.rules_panel.discard_open = false;
    st.rules_panel.discard_message.clear();
    reload_drafts(&mut st);
}

struct InspectorData {
    visible: bool,
    title: String,
    kind_options: Vec<String>,
    kind_index: i32,
    primary_label: String,
    primary_options: Vec<String>,
    primary_index: i32,
    param_a_label: String,
    param_a: String,
    param_b_label: String,
    param_b: String,
    param_column_label: String,
    param_column_options: Vec<String>,
    param_column_index: i32,
    param_op_options: Vec<String>,
    param_op_index: i32,
    has_target: bool,
    target_node_options: Vec<String>,
    target_node_index: i32,
    target_column_options: Vec<String>,
    target_column_index: i32,
    desc: String,
    hint: String,
    is_condition: bool,
    kind_label: String,
    docs_use: String,
    docs_example: String,
}

fn empty_inspector() -> InspectorData {
    InspectorData {
        visible: false,
        title: String::new(),
        kind_options: Vec::new(),
        kind_index: 0,
        primary_label: String::new(),
        primary_options: Vec::new(),
        primary_index: 0,
        param_a_label: String::new(),
        param_a: String::new(),
        param_b_label: String::new(),
        param_b: String::new(),
        param_column_label: String::new(),
        param_column_options: Vec::new(),
        param_column_index: 0,
        param_op_options: Vec::new(),
        param_op_index: 0,
        has_target: false,
        target_node_options: Vec::new(),
        target_node_index: 0,
        target_column_options: Vec::new(),
        target_column_index: 0,
        desc: String::new(),
        hint: String::new(),
        is_condition: false,
        kind_label: "验证器".into(),
        docs_use: String::new(),
        docs_example: String::new(),
    }
}

fn condition_leaf_desc(op: LeafOp) -> &'static str {
    match op {
        LeafOp::Eq => "列值等于给定值。",
        LeafOp::Ne => "列值不等于给定值。",
        LeafOp::Lt => "列值小于给定值。",
        LeafOp::Le => "列值小于等于给定值。",
        LeafOp::Gt => "列值大于给定值。",
        LeafOp::Ge => "列值大于等于给定值。",
        LeafOp::In => "列值属于候选集合。",
        LeafOp::Match => "列值匹配正则。",
    }
}

fn inspector_data(st: &AppState, specs: &[ColumnSpec], hint: &str) -> InspectorData {
    if let Some(l) = selected_condition(&st.rules_panel) {
        let mut out = empty_inspector();
        out.visible = true;
        out.is_condition = true;
        out.kind_label = "条件".into();
        out.title = l.column.clone();
        out.desc = condition_leaf_desc(l.op).to_string();
        out.docs_use = "行过滤条件：命中行才参与同组验证器。".into();
        out.docs_example = leaf_to_condition(l)
            .map(|c| format!("when({}, non_empty(<列>))", c.expr_string()))
            .unwrap_or_default();
        let options = leaf_op_options(column_class(specs, &l.column));
        out.kind_options = options.clone();
        out.kind_index = options.iter().position(|o| o == l.op.label()).unwrap_or(0) as i32;
        out.param_column_label = "列".into();
        out.param_column_options = scalar_columns(specs);
        out.param_column_index = index_of(&out.param_column_options, &l.column);
        match l.op {
            LeafOp::In => {
                out.param_a_label = "候选值（逗号分隔）".into();
                out.param_a = join_values(&l.values);
            }
            LeafOp::Match => {
                out.param_a_label = "正则".into();
                out.param_a = l.pattern.clone();
            }
            _ => {
                out.param_a_label = "值".into();
                out.param_a = l.value.clone();
            }
        }
        out.hint = hint.to_string();
        return out;
    }
    let Some(v) = selected_validator(&st.rules_panel) else { return empty_inspector() };
    let scope = st.rules_panel.tab;
    let class = match scope {
        RuleScope::Column => st.rules_panel.column.as_deref()
            .and_then(|c| column_class(specs, c)),
        RuleScope::Table => v.primary_column()
            .and_then(|c| column_class(specs, c)),
    };
    let keys: Vec<&str> = validator_keys(scope).into_iter()
        .filter(|k| *k == v.name() || scope == RuleScope::Table || validator_applies(k, class))
        .collect();
    let mut options: Vec<String> = keys.iter().map(|k| validator_label(k).to_string()).collect();
    options.extend(CONDITION_KEYS.iter().map(|k| condition_label(k).to_string()));
    let kind_index = keys.iter().position(|k| *k == v.name()).unwrap_or(0) as i32;
    let mut out = empty_inspector();
    out.visible = true;
    out.title = v.name().to_string();
    out.kind_options = options;
    out.kind_index = kind_index;
    out.desc = validator_desc(v.name()).to_string();
    out.docs_use = validator_tags(v.name()).to_string();
    out.docs_example = picker_example(v.name(), scope);
    out.hint = hint.to_string();

    if scope == RuleScope::Table && v.primary_column().is_some() {
        out.primary_label = "主列".to_string();
        let mut options: Vec<String> = scalar_columns(specs).into_iter()
            .filter(|c| validator_applies(v.name(), column_class(specs, c)))
            .collect();
        let current = v.primary_column().unwrap_or("");
        if !current.is_empty() && !options.iter().any(|c| c == current) {
            options.insert(0, current.to_string());
        }
        out.primary_index = index_of(&options, current);
        out.primary_options = options;
    }

    match v {
        RuleKind::Unique { .. } => {}
        RuleKind::NonEmpty { .. } => {}
        RuleKind::PositiveInt { .. } => {}
        RuleKind::UpperSnake { .. } => {}
        RuleKind::Range { min, max, .. } => {
            out.param_a_label = "下限".into(); out.param_a = min.clone();
            out.param_b_label = "上限".into(); out.param_b = max.clone();
        }
        RuleKind::Len { min, max, .. } => {
            out.param_a_label = "下限".into(); out.param_a = min.clone();
            out.param_b_label = "上限".into(); out.param_b = max.clone();
        }
        RuleKind::OneOf { values, .. } => {
            out.param_a_label = "候选值（逗号分隔）".into();
            out.param_a = join_values(values);
        }
        RuleKind::Matches { pattern, .. } => {
            out.param_a_label = "正则".into(); out.param_a = pattern.clone();
        }
        RuleKind::RefExists { target_node, target_column, .. } => {
            out.has_target = true;
            let targets = target_nodes(st);
            out.target_node_index = index_of(&targets, target_node);
            out.target_node_options = targets;
            out.target_column_options = target_columns(st, target_node);
            out.target_column_index = index_of(&out.target_column_options, target_column);
        }
        RuleKind::Compare { op, other, .. } => {
            out.param_column_label = "比较列".into();
            let mut options: Vec<String> = scalar_columns(specs).into_iter()
                .filter(|c| Some(c.as_str()) != v.primary_column())
                .filter(|c| validator_applies("compare", column_class(specs, c)))
                .collect();
            if !other.is_empty() && !options.iter().any(|c| c == other) {
                options.insert(0, other.clone());
            }
            out.param_column_index = index_of(&options, other);
            out.param_column_options = options;
            out.param_op_options = ["<", "<=", ">", ">=", "==", "!="]
                .iter().map(|s| s.to_string()).collect();
            out.param_op_index = out.param_op_options.iter()
                .position(|s| s == op.as_str()).unwrap_or(0) as i32;
        }
        RuleKind::AllNonEmpty { columns } => {
            out.param_a_label = "列（逗号分隔）".into(); out.param_a = columns.join(", ");
        }
        RuleKind::AtLeastOne { columns } => {
            out.param_a_label = "列（逗号分隔）".into(); out.param_a = columns.join(", ");
        }
        RuleKind::ExactlyOne { columns } => {
            out.param_a_label = "列（逗号分隔）".into(); out.param_a = columns.join(", ");
        }
        RuleKind::RowCount { min, max } => {
            out.param_a_label = "下限".into(); out.param_a = min.clone();
            out.param_b_label = "上限".into(); out.param_b = max.clone();
        }
        RuleKind::IdContiguous => {}
        RuleKind::UniqueCombination { columns } => {
            out.param_a_label = "列（逗号分隔）".into(); out.param_a = columns.join(", ");
        }
        RuleKind::NoEmptyRows => {
            out.desc = "no_empty_rows() 尚未实现".into();
        }
    }
    out
}

fn palette_detail(st: &AppState) -> (String, String, String, String, Vec<PickerParam>, String, bool) {
    let scope = st.rules_panel.tab;
    let Some(key) = st.rules_panel.palette_selected.as_deref() else {
        return (String::new(), String::new(), String::new(), String::new(), Vec::new(), String::new(), false);
    };
    (
        key.to_string(),
        picker_signature(key, scope).to_string(),
        validator_desc(key).to_string(),
        validator_tags(key).to_string(),
        picker_params(key, scope),
        picker_example(key, scope),
        true,
    )
}

/// 切换作用域后重新加载该作用域的全部规则草稿。
fn load_scope(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    st.rules_panel.selected_rule = 0;
    reload_drafts(&mut st);
}

pub fn push(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    ui_h.set_dlg_rules_open(st.rules_panel.open);
    if !st.rules_panel.open { return; }

    normalize_selection(&mut st);

    let group = st.rules_panel.group.clone();
    let name = st.rules_panel.name.clone();
    let mode = node_mode(&st, &group, &name);
    let rules = node_rules(&st, &group, &name);
    let issues = node_issues(&st, &group, &name);
    let specs = st.engine.node_column_classes(&group, &name);
    let target = current_target(&st);
    let scope = st.rules_panel.tab;
    let anchor = current_anchor(&st);

    let mode_label = match mode {
        Some(NodeMode::Table) => "Table",
        Some(NodeMode::Enum) => "Enum",
        _ => "",
    };
    ui_h.set_ru_node_label(SharedString::from(format!("{}/{}  ({})", group, name, mode_label)));
    ui_h.set_ru_has_table_tab(mode == Some(NodeMode::Table));
    ui_h.set_ru_tab_index(if scope == RuleScope::Table { 1 } else { 0 });

    // 左：列清单（表级 Tab 里不显示，但数据仍推，便于切回）
    let builtin_unique = |col: &str| mode.map_or(false, |m| {
        builtin_rules(m).iter().any(|r| {
            r.target == RuleTarget::Column(col.to_string())
                && matches!(&r.expr, RuleExpr::Validator(RuleKind::Unique { .. }))
        })
    });
    let columns: Vec<RuleColumnRow> = specs.iter().map(|(col, class)| {
        let count = rules.iter().filter(|r| r.target == RuleTarget::Column(col.clone())).count();
        let unsupported = class.is_none();
        let badge = if unsupported { "不支持规则".to_string() }
            else if count > 0 { format!("{} 条", count) }
            else if builtin_unique(col) { "系统唯一".to_string() }
            else { String::new() };
        RuleColumnRow {
            name: SharedString::from(col.clone()),
            badge: SharedString::from(badge),
            unsupported,
            selected: scope == RuleScope::Column && st.rules_panel.column.as_deref() == Some(col.as_str()),
        }
    }).collect();
    ui_h.set_ru_columns(ModelRc::new(VecModel::from(columns)));

    let class = anchor.as_deref().and_then(|a| column_class(&specs, a));
    ui_h.set_ru_column_title(SharedString::from(match scope {
        RuleScope::Column => format!(
            "列 \"{}\"  ·  {}", anchor.as_deref().unwrap_or(""), class_label(class)
        ),
        RuleScope::Table => "表级规则（逐行多列 / 全表）".to_string(),
    }));

    // 系统规则只读列表：节点级内置规则全量展示，不按当前列过滤。
    // 自定义规则全部由 tree 里的卡片行渲染。
    let mode_rules = mode.map(builtin_rules).unwrap_or_default();
    let mut cards: Vec<RuleCard> = Vec::new();
    for b in mode_rules.iter() {
        cards.push(RuleCard {
            num: SharedString::from("·"),
            title: SharedString::from(b.body_string()),
            level: SharedString::from("系统"),
            is_warning: false, is_info: false, selected: false, is_builtin: true,
            can_up: false, can_down: false,
        });
    }
    let system_text = cards.iter()
        .map(|r| r.title.to_string())
        .collect::<Vec<_>>()
        .join(" · ");
    ui_h.set_ru_rule_system_text(SharedString::from(system_text));
    ui_h.set_ru_rule_current_card(-1);
    ui_h.set_ru_rule_can_delete(false);
    ui_h.set_ru_rule_can_up(false);
    ui_h.set_ru_rule_can_down(false);
    ui_h.set_ru_rules(ModelRc::new(VecModel::from(cards)));

    // 草稿数量变化后校正激活卡片。
    let count = st.rules_panel.drafts.len() as i32;
    if count == 0 {
        st.rules_panel.selected_rule = -1;
        st.rules_panel.selected = None;
    } else if st.rules_panel.selected_rule < 0 {
        st.rules_panel.selected_rule = 0;
    } else if st.rules_panel.selected_rule >= count {
        st.rules_panel.selected_rule = count - 1;
    }

    // 统一使用逐行组合树；不再区分简单 / 高级模式。
    let rows = build_rows(&st, &specs);
    let row_keys: Vec<RowKey> = rows.iter().map(|r| r.key.clone()).collect();
    let row_kinds: Vec<String> = rows.iter().map(|r| r.row.kind.to_string()).collect();
    ui_h.set_ru_tree(ModelRc::new(VecModel::from(
        rows.into_iter().map(|r| r.row).collect::<Vec<_>>(),
    )));
    ui_h.set_ru_cond_column_options(ModelRc::new(VecModel::from(
        scalar_columns(&specs).into_iter().map(SharedString::from).collect::<Vec<_>>(),
    )));
    st.rules_panel.row_keys = row_keys;
    st.rules_panel.row_kinds = row_kinds;

    let (form_ok, hint) = form_status(&st);
    let editor_title = "规则编辑".to_string();
    let editor_path = match &target {
        Some(RuleTarget::Column(c)) => format!("column {}:", c),
        Some(RuleTarget::Table) => "table:".to_string(),
        None => String::new(),
    };
    ui_h.set_ru_editor_title(SharedString::from(editor_title));
    ui_h.set_ru_editor_path(SharedString::from(editor_path));
    let active_level = if st.rules_panel.selected_rule >= 0 {
        st.rules_panel.drafts.get(st.rules_panel.selected_rule as usize)
            .map(|d| d.level)
            .unwrap_or(Severity::Error)
    } else {
        Severity::Error
    };
    ui_h.set_ru_level_index(level_index(active_level));
    ui_h.set_ru_can_save(st.rules_panel.dirty && form_ok);
    ui_h.set_ru_save_label(SharedString::from("保存规则"));
    ui_h.set_ru_preview(SharedString::from(""));
    ui_h.set_ru_hint(SharedString::from(hint.clone()));
    ui_h.set_ru_can_confirm(form_ok);
    ui_h.set_ru_discard_open(st.rules_panel.discard_open);
    ui_h.set_ru_discard_message(SharedString::from(st.rules_panel.discard_message.clone()));
    let can_add = match scope {
        RuleScope::Column => class.is_some(),
        RuleScope::Table => mode == Some(NodeMode::Table),
    };
    ui_h.set_ru_new_rule_selected(false);
    ui_h.set_ru_can_new_rule(can_add);
    ui_h.set_ru_can_add_node(can_add);
    ui_h.set_ru_editor_mode(1);
    ui_h.set_ru_simple_available(false);

    // 快速添加：默认选中的函数下拉 + +
    let mut quick_keys: Vec<&str> = validator_keys(scope).into_iter()
        .filter(|k| scope == RuleScope::Table || validator_applies(k, class))
        .collect();
    quick_keys.extend(CONDITION_KEYS.iter().copied());
    if !quick_keys.iter().any(|k| **k == st.rules_panel.quick_function) {
        st.rules_panel.quick_function = quick_keys.first()
            .map(|k| k.to_string())
            .unwrap_or_else(|| "unique".to_string());
    }
    let quick_index = quick_keys.iter()
        .position(|k| **k == st.rules_panel.quick_function)
        .unwrap_or(0) as i32;
    ui_h.set_ru_quick_function_options(ModelRc::new(VecModel::from(
        quick_keys.iter()
            .map(|k| SharedString::from(if CONDITION_KEYS.contains(k) {
                condition_label(k)
            } else {
                validator_label(k)
            }))
            .collect::<Vec<_>>(),
    )));
    ui_h.set_ru_quick_function_index(quick_index);

    // Inspector
    let inspector = inspector_data(&st, &specs, &hint);
    ui_h.set_ru_inspector_visible(inspector.visible);
    ui_h.set_ru_inspector_title(SharedString::from(inspector.title));
    ui_h.set_ru_inspector_kind_options(ModelRc::new(VecModel::from(
        inspector.kind_options.iter().map(|s| SharedString::from(s.clone())).collect::<Vec<_>>(),
    )));
    ui_h.set_ru_inspector_kind_index(inspector.kind_index);
    ui_h.set_ru_inspector_primary_label(SharedString::from(inspector.primary_label));
    ui_h.set_ru_inspector_primary_options(ModelRc::new(VecModel::from(
        inspector.primary_options.iter().map(|s| SharedString::from(s.clone())).collect::<Vec<_>>(),
    )));
    ui_h.set_ru_inspector_primary_index(inspector.primary_index);
    ui_h.set_ru_inspector_param_a_label(SharedString::from(inspector.param_a_label));
    ui_h.set_ru_inspector_param_a(SharedString::from(inspector.param_a));
    ui_h.set_ru_inspector_param_b_label(SharedString::from(inspector.param_b_label));
    ui_h.set_ru_inspector_param_b(SharedString::from(inspector.param_b));
    ui_h.set_ru_inspector_param_column_label(SharedString::from(inspector.param_column_label));
    ui_h.set_ru_inspector_param_column_options(ModelRc::new(VecModel::from(
        inspector.param_column_options.iter()
            .map(|s| SharedString::from(s.clone())).collect::<Vec<_>>(),
    )));
    ui_h.set_ru_inspector_param_column_index(inspector.param_column_index);
    ui_h.set_ru_inspector_param_op_options(ModelRc::new(VecModel::from(
        inspector.param_op_options.iter().map(|s| SharedString::from(s.clone())).collect::<Vec<_>>(),
    )));
    ui_h.set_ru_inspector_param_op_index(inspector.param_op_index);
    ui_h.set_ru_inspector_has_target(inspector.has_target);
    ui_h.set_ru_inspector_target_node_options(ModelRc::new(VecModel::from(
        inspector.target_node_options.iter().map(|s| SharedString::from(s.clone())).collect::<Vec<_>>(),
    )));
    ui_h.set_ru_inspector_target_node_index(inspector.target_node_index);
    ui_h.set_ru_inspector_target_column_options(ModelRc::new(VecModel::from(
        inspector.target_column_options.iter().map(|s| SharedString::from(s.clone())).collect::<Vec<_>>(),
    )));
    ui_h.set_ru_inspector_target_column_index(inspector.target_column_index);
    ui_h.set_ru_inspector_desc(SharedString::from(inspector.desc));
    ui_h.set_ru_inspector_hint(SharedString::from(inspector.hint));
    ui_h.set_ru_inspector_is_condition(inspector.is_condition);
    ui_h.set_ru_inspector_kind_label(SharedString::from(inspector.kind_label));
    ui_h.set_ru_inspector_docs_use(SharedString::from(inspector.docs_use));
    ui_h.set_ru_inspector_docs_example(SharedString::from(inspector.docs_example));

    // 函数选择器
    let items = palette_items(&st);
    let (pal_name, pal_sig, pal_desc, pal_tags, pal_params, pal_example, pal_can_add) =
        palette_detail(&st);
    ui_h.set_ru_palette_open(st.rules_panel.palette_open);
    let replacing = st.rules_panel.palette_replace.is_some();
    let palette_verb = if replacing { "切换校验项" } else { "添加校验项" };
    ui_h.set_ru_palette_title(SharedString::from(match scope {
        RuleScope::Column => format!(
            "{} · 列规则 · {}",
            palette_verb,
            st.rules_panel.column.as_deref().unwrap_or("")
        ),
        RuleScope::Table => format!("{} · 表级规则", palette_verb),
    }));
    ui_h.set_ru_palette_action_label(SharedString::from(if replacing { "替换" } else { "添加" }));
    ui_h.set_ru_palette_items(ModelRc::new(VecModel::from(items)));
    ui_h.set_ru_palette_name(SharedString::from(pal_name));
    ui_h.set_ru_palette_signature(SharedString::from(pal_sig));
    ui_h.set_ru_palette_desc(SharedString::from(pal_desc));
    ui_h.set_ru_palette_tags(SharedString::from(pal_tags));
    ui_h.set_ru_palette_params(ModelRc::new(VecModel::from(pal_params)));
    ui_h.set_ru_palette_example(SharedString::from(pal_example));
    ui_h.set_ru_palette_can_add(pal_can_add);
    ui_h.set_ru_palette_filter_index(st.rules_panel.palette_filter);
    ui_h.set_ru_palette_hint(SharedString::from(match scope {
        RuleScope::Column => "当前是列规则：只显示列域函数。行级 / 全表函数切到「表级规则」Tab 后可选。".to_string(),
        RuleScope::Table => "表级规则：列域函数添加后需在 inspector 选择主列；行级 / 全表函数直接选列列表。".to_string(),
    }));

    // 结果区
    let issue_rows: Vec<RuleIssueRow> = issues.iter().map(|e| RuleIssueRow {
        severity: SharedString::from(match e.severity {
            Severity::Error => "error", Severity::Warning => "warning", Severity::Info => "info",
        }),
        text: SharedString::from(format_validation_log(&group, &name, e)),
    }).collect();
    let first_issue = issues.first()
        .map(|e| format_validation_log(&group, &name, e))
        .unwrap_or_default();
    ui_h.set_ru_first_issue(SharedString::from(first_issue));
    ui_h.set_ru_issues(ModelRc::new(VecModel::from(issue_rows)));
    ui_h.set_ru_error_count(issues.iter().filter(|e| e.severity == Severity::Error).count() as i32);
    ui_h.set_ru_warning_count(issues.iter().filter(|e| e.severity == Severity::Warning).count() as i32);
    ui_h.set_ru_info_count(issues.iter().filter(|e| e.severity == Severity::Info).count() as i32);
}

fn reset_draft(state: &Rc<RefCell<AppState>>) {
    let mut st = state.borrow_mut();
    reload_drafts(&mut st);
    st.rules_panel.palette_open = false;
    st.rules_panel.palette_target = None;
    st.rules_panel.palette_replace = None;
    st.rules_panel.palette_selected = palette_first(st.rules_panel.tab);
    st.rules_panel.palette_search.clear();
    st.rules_panel.palette_filter = 0;
}

/// 回调里按行号取「规则 + 草稿内路径」。
fn row_key_at(state: &Rc<RefCell<AppState>>, i: i32) -> Option<RowKey> {
    if i < 0 { return None; }
    state.borrow().rules_panel.row_keys.get(i as usize).cloned()
}

fn draft_of_mut<'a>(st: &'a mut RulesPanelState, key: &RowKey) -> Option<&'a mut DraftExpr> {
    st.drafts.get_mut(key.rule).map(|d| &mut d.draft)
}

// ── 回调注册 ────────────────────────────────────────────────────────────

pub fn wire(ui_h: &AppWindow, state: &Rc<RefCell<AppState>>) {
    // Tab
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tab_changed(move |i| {
            let want = if i == 1 { RuleScope::Table } else { RuleScope::Column };
            let (dirty, current, mode) = {
                let st = s.borrow();
                (
                    st.rules_panel.dirty,
                    st.rules_panel.tab,
                    node_mode(&st, &st.rules_panel.group, &st.rules_panel.name),
                )
            };
            if want == current { return; }
            if want == RuleScope::Table && mode != Some(NodeMode::Table) { return; }
            {
                let mut st = s.borrow_mut();
                if dirty {
                    st.rules_panel.pending_switch = Some(PendingSwitch::Tab(want));
                    st.rules_panel.discard_open = true;
                    st.rules_panel.discard_message = format!(
                        "切换到「{}」会丢弃当前未保存的规则。",
                        if want == RuleScope::Table { "表级规则" } else { "列规则" },
                    );
                } else {
                    st.rules_panel.tab = want;
                }
            }
            if dirty {
                if let Some(ui) = weak.upgrade() { push(&ui, &s); }
                return;
            }
            load_scope(&s);
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 选列
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_select_column(move |i| {
            let specs = {
                let st = s.borrow();
                st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name)
            };
            if i < 0 || i as usize >= specs.len() { return; }
            let col = specs[i as usize].0.clone();
            let (dirty, same) = {
                let st = s.borrow();
                (
                    st.rules_panel.dirty,
                    st.rules_panel.tab == RuleScope::Column
                        && st.rules_panel.column.as_deref() == Some(col.as_str()),
                )
            };
            if same { return; }
            {
                let mut st = s.borrow_mut();
                if dirty {
                    st.rules_panel.pending_switch = Some(PendingSwitch::Column(col.clone()));
                    st.rules_panel.discard_open = true;
                    st.rules_panel.discard_message =
                        format!("切换到列「{}」会丢弃当前未保存的规则。", col);
                } else {
                    st.rules_panel.tab = RuleScope::Column;
                    st.rules_panel.column = Some(col);
                }
            }
            if dirty {
                if let Some(ui) = weak.upgrade() { push(&ui, &s); }
                return;
            }
            load_scope(&s);
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 丢弃未保存改动并切换
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_discard_confirm(move || {
            let target = s.borrow_mut().rules_panel.pending_switch.take();
            let Some(target) = target else { return; };
            {
                let mut st = s.borrow_mut();
                match target {
                    PendingSwitch::Tab(scope) => {
                        let mode = node_mode(&st, &st.rules_panel.group, &st.rules_panel.name);
                        st.rules_panel.tab = if scope == RuleScope::Table && mode != Some(NodeMode::Table) {
                            RuleScope::Column
                        } else {
                            scope
                        };
                    }
                    PendingSwitch::Column(col) => {
                        st.rules_panel.tab = RuleScope::Column;
                        st.rules_panel.column = Some(col);
                    }
                }
                st.rules_panel.selected_rule = 0;
                st.rules_panel.discard_open = false;
                st.rules_panel.discard_message.clear();
                reload_drafts(&mut st);
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_discard_cancel(move || {
            {
                let mut st = s.borrow_mut();
                st.rules_panel.pending_switch = None;
                st.rules_panel.discard_open = false;
                st.rules_panel.discard_message.clear();
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 新建 / 选择 / 删除 / 折叠 / 级别
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_new_rule(move || {
            let (specs, primary) = {
                let st = s.borrow();
                (
                    st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name),
                    current_anchor(&st),
                )
            };
            {
                let mut st = s.borrow_mut();
                let node = new_default_node(&st, &specs, primary.as_deref());
                let idx = st.rules_panel.drafts.len();
                // 草稿根永远是组：+行 / +组 / 合并都按「根组 + 子节点」寻址。
                st.rules_panel.drafts.push(RuleDraft::new(
                    group_with_one(node), Severity::Error, None,
                ));
                st.rules_panel.selected_rule = idx as i32;
                st.rules_panel.selected = Some(RowKey { rule: idx, path: vec![0] });
                st.rules_panel.dirty = true;
                if let Some(d) = st.rules_panel.drafts.get_mut(idx) { d.dirty = true; }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_select_rule(move |i| {
            {
                let mut st = s.borrow_mut();
                if i < 0 || i as usize >= st.rules_panel.drafts.len() { return; }
                let idx = i as usize;
                st.rules_panel.selected_rule = i;
                st.rules_panel.selected = first_validator_path(
                    &st.rules_panel.drafts[idx].draft, &mut Vec::new(),
                ).map(|path| RowKey { rule: idx, path });
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_delete_rule(move |i| {
            {
                let mut st = s.borrow_mut();
                if i < 0 || i as usize >= st.rules_panel.drafts.len() { return; }
                st.rules_panel.drafts.remove(i as usize);
                let len = st.rules_panel.drafts.len() as i32;
                st.rules_panel.selected_rule = if len == 0 { -1 } else { i.min(len - 1) };
                st.rules_panel.selected = None;
                st.rules_panel.dirty = true;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_rule_toggle(move |i| {
            {
                let mut st = s.borrow_mut();
                let idx = i.max(0) as usize;
                let collapsed = if let Some(d) = st.rules_panel.drafts.get_mut(idx) {
                    d.collapsed = !d.collapsed;
                    d.collapsed
                } else {
                    false
                };
                if collapsed && st.rules_panel.selected.as_ref().map(|k| k.rule) == Some(idx) {
                    st.rules_panel.selected = None;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_rule_level(move |i, picked| {
            let level = match picked.as_str() {
                "警告" => Severity::Warning,
                "信息" => Severity::Info,
                _ => Severity::Error,
            };
            {
                let mut st = s.borrow_mut();
                if let Some(d) = st.rules_panel.drafts.get_mut(i.max(0) as usize) {
                    d.level = level;
                    d.dirty = true;
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_move_up(move |i| move_rule(&s, &weak, i, true));
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_move_down(move |i| move_rule(&s, &weak, i, false));
    }

    // 单模式 UI 已移除；保留回调避免旧属性接线报错。
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_editor_mode_changed(move |_| {
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_simple_add_validator(move || {
            {
                let mut st = s.borrow_mut();
                st.rules_panel.palette_target = Some(RowKey {
                    rule: st.rules_panel.selected_rule.max(0) as usize,
                    path: Vec::new(),
                });
                st.rules_panel.palette_replace = None;
                st.rules_panel.palette_open = true;
                st.rules_panel.palette_selected = palette_first(st.rules_panel.tab);
                st.rules_panel.palette_search.clear();
                st.rules_panel.palette_filter = 0;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 树：选择 / 删除 / 调序 / 组逻辑
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_select(move |i| {
            let Some(key) = row_key_at(&s, i) else { return };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = key.rule as i32;
                st.rules_panel.selected = Some(key);
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_delete(move |i| {
            let Some(key) = row_key_at(&s, i) else { return };
            if key.path.is_empty() { return; }
            let mut st = s.borrow_mut();
            let changed = draft_of_mut(&mut st.rules_panel, &key)
                .and_then(|d| remove_node(d, &key.path))
                .is_some();
            if changed {
                if let Some(d) = draft_of_mut(&mut st.rules_panel, &key) { prune_empty_groups(d); }
                st.rules_panel.selected_rule = key.rule as i32;
                st.rules_panel.selected = None;
                st.rules_panel.mark_dirty();
            }
            drop(st);
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    // 树：+行 / +组 / 合并 / 分散
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_insert_row(move |i| {
            let Some(key) = row_key_at(&s, i) else { return };
            let (specs, primary) = {
                let st = s.borrow();
                (
                    st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name),
                    current_anchor(&st),
                )
            };
            let node = {
                let st = s.borrow();
                new_default_node(&st, &specs, primary.as_deref())
            };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = key.rule as i32;
                let added = draft_of_mut(&mut st.rules_panel, &key)
                    .and_then(|d| insert_sibling(d, &key.path, node));
                if let Some(path) = added {
                    st.rules_panel.selected = Some(RowKey { rule: key.rule, path });
                    st.rules_panel.mark_dirty();
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_insert_group(move |i| {
            let Some(key) = row_key_at(&s, i) else { return };
            let (specs, primary) = {
                let st = s.borrow();
                (
                    st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name),
                    current_anchor(&st),
                )
            };
            let group = {
                let st = s.borrow();
                new_default_group(&st, &specs, primary.as_deref())
            };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = key.rule as i32;
                let added = draft_of_mut(&mut st.rules_panel, &key)
                    .and_then(|d| insert_sibling(d, &key.path, group));
                if let Some(mut path) = added {
                    path.push(0);
                    st.rules_panel.selected = Some(RowKey { rule: key.rule, path });
                    st.rules_panel.mark_dirty();
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_merge_root(move || {
            {
                let mut st = s.borrow_mut();
                let merged = st.rules_panel.active_draft_mut()
                    .map(|d| merge_root(d))
                    .unwrap_or(false);
                if merged {
                    st.rules_panel.selected = None;
                    st.rules_panel.mark_dirty();
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_dissolve(move |i| {
            let Some(key) = row_key_at(&s, i) else { return };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = key.rule as i32;
                let moved = draft_of_mut(&mut st.rules_panel, &key)
                    .and_then(|d| dissolve_group(d, &key.path));
                if let Some(path) = moved {
                    st.rules_panel.selected = Some(RowKey { rule: key.rule, path });
                    st.rules_panel.mark_dirty();
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_move_up(move |i| move_tree_node(&s, &weak, i, true));
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_move_down(move |i| move_tree_node(&s, &weak, i, false));
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_set_group_logic(move |i, label| {
            let Some(key) = row_key_at(&s, i) else { return };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = key.rule as i32;
                let op = LogicOp::from_label(label.as_str());
                let changed = draft_of_mut(&mut st.rules_panel, &key)
                    .map(|d| set_connector_at(d, &key.path, op))
                    .unwrap_or(false);
                if changed { st.rules_panel.mark_dirty(); }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 树：添加节点
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_add_validator(move |i| {
            let Some(key) = row_key_at(&s, i) else { return };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = key.rule as i32;
                st.rules_panel.palette_target = Some(key);
                st.rules_panel.palette_replace = None;
                st.rules_panel.palette_open = true;
                st.rules_panel.palette_selected = palette_first(st.rules_panel.tab);
                st.rules_panel.palette_search.clear();
                st.rules_panel.palette_filter = 0;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_add_group(move |i| {
            let Some(key) = row_key_at(&s, i) else { return };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = key.rule as i32;
                let added = draft_of_mut(&mut st.rules_panel, &key)
                    .map(|d| push_child(d, &key.path, empty_group()))
                    .unwrap_or(false);
                if added {
                    let count = draft_of_mut(&mut st.rules_panel, &key)
                        .and_then(|d| draft_node(d, &key.path))
                        .and_then(|n| match n {
                            DraftExpr::Group { children, .. } => Some(children.len()),
                            _ => None,
                        })
                        .unwrap_or(0);
                    if count > 0 {
                        let mut path = key.path.clone();
                        path.push(count - 1);
                        st.rules_panel.selected = Some(RowKey { rule: key.rule, path });
                        st.rules_panel.mark_dirty();
                    }
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_add_condition(move |i| {
            let Some(key) = row_key_at(&s, i) else { return };
            let (specs, anchor) = {
                let st = s.borrow();
                (st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name),
                 current_anchor(&st))
            };
            let leaf = default_leaf(&specs, anchor.as_deref());
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = key.rule as i32;
                let added = draft_of_mut(&mut st.rules_panel, &key)
                    .map(|d| push_child(d, &key.path, DraftExpr::Condition(leaf)))
                    .unwrap_or(false);
                if added { st.rules_panel.mark_dirty(); }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_tree_add_condition_group(move |i| {
            let Some(key) = row_key_at(&s, i) else { return };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = key.rule as i32;
                let added = draft_of_mut(&mut st.rules_panel, &key)
                    .map(|d| push_child(d, &key.path, empty_group()))
                    .unwrap_or(false);
                if added { st.rules_panel.mark_dirty(); }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 查询式：快速添加函数下拉 + +
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_query_quick_function(move |label| {
            let (scope, specs, class) = {
                let st = s.borrow();
                let specs = st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name);
                let scope = st.rules_panel.tab;
                let class = if scope == RuleScope::Column {
                    st.rules_panel.column.as_deref().and_then(|c| column_class(&specs, c))
                } else {
                    None
                };
                (scope, specs, class)
            };
            let _ = specs;
            let mut keys: Vec<&str> = validator_keys(scope).into_iter()
                .filter(|k| scope == RuleScope::Table || validator_applies(k, class))
                .collect();
            keys.extend(CONDITION_KEYS.iter().copied());
            let key = keys.into_iter().find(|k| {
                if CONDITION_KEYS.contains(k) {
                    label.as_str() == condition_label(k)
                } else {
                    label.as_str() == validator_label(k)
                }
            });
            let Some(key) = key else { return };
            s.borrow_mut().rules_panel.quick_function = key.to_string();
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_query_quick_add(move |i| {
            let Some(row) = row_key_at(&s, i) else { return };
            let (func, primary, scope, specs) = {
                let st = s.borrow();
                let scope = st.rules_panel.tab;
                let specs = st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name);
                let primary = current_anchor(&st);
                (st.rules_panel.quick_function.clone(), primary, scope, specs)
            };
            let node = if CONDITION_KEYS.contains(&func.as_str()) {
                DraftExpr::Condition(default_condition(&func, &specs, primary.as_deref()))
            } else {
                let Some(key_ref) = validator_keys(scope).into_iter().find(|k| *k == func) else {
                    return;
                };
                let st = s.borrow();
                DraftExpr::Validator(default_validator(
                    key_ref, scope, primary.as_deref(), &specs, &st,
                ))
            };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = row.rule as i32;
                let mut new_path = row.path.clone();
                let mut added = false;
                if let Some(draft) = draft_of_mut(&mut st.rules_panel, &row) {
                    if let Some(DraftExpr::Group { children, .. }) =
                        draft_node_mut(draft, &row.path)
                    {
                        children.push(node);
                        new_path.push(children.len() - 1);
                        added = true;
                    }
                }
                if added {
                    st.rules_panel.selected = Some(RowKey { rule: row.rule, path: new_path });
                    st.rules_panel.mark_dirty();
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 查询式：点击函数名切换函数
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_query_change_function(move |i| {
            let Some(row) = row_key_at(&s, i) else { return };
            let current = {
                let st = s.borrow();
                let Some(draft) = st.rules_panel.drafts.get(row.rule).map(|d| &d.draft) else { return };
                match draft_node(draft, &row.path) {
                    Some(DraftExpr::Validator(v)) => v.name().to_string(),
                    _ => return,
                }
            };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = row.rule as i32;
                st.rules_panel.palette_target = None;
                st.rules_panel.palette_replace = Some(row);
                st.rules_panel.palette_open = true;
                st.rules_panel.palette_selected = Some(current);
                st.rules_panel.palette_search.clear();
                st.rules_panel.palette_filter = 0;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 查询式：行尾 and/or 下拉，改父组 operator
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_query_set_row_logic(move |i, label| {
            let Some(row) = row_key_at(&s, i) else { return };
            let op = if label.contains("or") { LogicOp::Or } else { LogicOp::And };
            {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = row.rule as i32;
                let changed = draft_of_mut(&mut st.rules_panel, &row)
                    .map(|d| set_connector_at(d, &row.path, op))
                    .unwrap_or(false);
                if changed { st.rules_panel.mark_dirty(); }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 查询式：组外 (+) 加兄弟组
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_query_add_sibling_group(move |i| {
            let Some(row) = row_key_at(&s, i) else { return };
            let (specs, primary) = {
                let st = s.borrow();
                (
                    st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name),
                    current_anchor(&st),
                )
            };
            let new_path = {
                let mut st = s.borrow_mut();
                st.rules_panel.selected_rule = row.rule as i32;
                let group = new_default_group(&st, &specs, primary.as_deref());
                let added = draft_of_mut(&mut st.rules_panel, &row)
                    .and_then(|d| insert_sibling(d, &row.path, group));
                if let Some(p) = &added {
                    let mut selected = p.clone();
                    selected.push(0);
                    st.rules_panel.selected = Some(RowKey { rule: row.rule, path: selected });
                    st.rules_panel.mark_dirty();
                }
                added
            };
            if new_path.is_some() {
                if let Some(ui) = weak.upgrade() { push(&ui, &s); }
            }
        });
    }

    // 条件叶子内联编辑
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_cond_set_column(move |i, label| {
            if i < 0 { return; }
            let specs = {
                let st = s.borrow();
                st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name)
            };
            {
                let mut st = s.borrow_mut();
                if let Some(l) = condition_leaf_mut(&mut st.rules_panel, i as usize) {
                    l.column = label.to_string();
                    let options = leaf_op_options(column_class(&specs, &l.column));
                    if !options.iter().any(|o| o == l.op.label()) { l.op = LeafOp::Eq; }
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_cond_set_op(move |i, label| {
            if i < 0 { return; }
            let Some(op) = LeafOp::parse_label(label.as_str()) else { return };
            {
                let mut st = s.borrow_mut();
                if let Some(l) = condition_leaf_mut(&mut st.rules_panel, i as usize) {
                    l.op = op;
                    match op {
                        LeafOp::In => {
                            if l.values.is_empty() && !l.value.trim().is_empty() {
                                l.values.push(l.value.clone());
                            }
                        }
                        LeafOp::Match => {}
                        _ => {
                            if l.value.is_empty() {
                                l.value = l.values.first().cloned().unwrap_or_default();
                            }
                        }
                    }
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_cond_set_value(move |i, text| {
            if i < 0 { return; }
            {
                let mut st = s.borrow_mut();
                if let Some(l) = condition_leaf_mut(&mut st.rules_panel, i as usize) {
                    l.value = text.to_string();
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_cond_add_value(move |i, text| {
            let text = text.trim().to_string();
            if text.is_empty() { return; }
            if i < 0 { return; }
            {
                let mut st = s.borrow_mut();
                if let Some(l) = condition_leaf_mut(&mut st.rules_panel, i as usize) {
                    if !l.values.contains(&text) { l.values.push(text.clone()); }
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_cond_remove_value(move |i, index| {
            if i < 0 { return; }
            {
                let mut st = s.borrow_mut();
                if let Some(l) = condition_leaf_mut(&mut st.rules_panel, i as usize) {
                    if (index as usize) < l.values.len() { l.values.remove(index as usize); }
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_cond_set_pattern(move |i, text| {
            if i < 0 { return; }
            {
                let mut st = s.borrow_mut();
                if let Some(l) = condition_leaf_mut(&mut st.rules_panel, i as usize) {
                    l.pattern = text.to_string();
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 函数选择器
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_palette_select(move |i| {
            let items = { let st = s.borrow(); palette_items(&st) };
            if i < 0 || i as usize >= items.len() { return; }
            let item = &items[i as usize];
            if item.is_group { return; }
            s.borrow_mut().rules_panel.palette_selected = Some(item.key.to_string());
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_palette_add(move || add_palette_item(&s, &weak));
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_palette_pick(move |i| {
            let items = { let st = s.borrow(); palette_items(&st) };
            if i < 0 || i as usize >= items.len() { return; }
            let item = &items[i as usize];
            if item.is_group { return; }
            s.borrow_mut().rules_panel.palette_selected = Some(item.key.to_string());
            add_palette_item(&s, &weak);
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_palette_cancel(move || {
            {
                let mut st = s.borrow_mut();
                st.rules_panel.palette_open = false;
                st.rules_panel.palette_target = None;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_palette_search(move |text| {
            {
                let mut st = s.borrow_mut();
                st.rules_panel.palette_search = text.to_string();
                let items = palette_items(&st);
                let keep = st.rules_panel.palette_selected.as_deref().map_or(false, |k| {
                    items.iter().any(|it| !it.is_group && it.key.as_str() == k)
                });
                if !keep {
                    st.rules_panel.palette_selected = items.iter()
                        .find(|it| !it.is_group)
                        .map(|it| it.key.to_string());
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_palette_filter(move |i| {
            {
                let mut st = s.borrow_mut();
                st.rules_panel.palette_filter = i;
                let items = palette_items(&st);
                let keep = st.rules_panel.palette_selected.as_deref().map_or(false, |k| {
                    items.iter().any(|it| !it.is_group && it.key.as_str() == k)
                });
                if !keep {
                    st.rules_panel.palette_selected = items.iter()
                        .find(|it| !it.is_group)
                        .map(|it| it.key.to_string());
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // Inspector：验证器参数
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_inspector_set_kind(move |label| {
            let specs = {
                let st = s.borrow();
                st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name)
            };
            let is_condition = {
                let st = s.borrow();
                selected_condition(&st.rules_panel).is_some()
            };
            if is_condition {
                let Some(op) = LeafOp::parse_label(label.as_str()) else { return };
                {
                    let mut st = s.borrow_mut();
                    if let Some(l) = selected_condition_mut(&mut st.rules_panel) {
                        l.op = op;
                        match op {
                            LeafOp::In => {
                                if l.values.is_empty() && !l.value.trim().is_empty() {
                                    l.values.push(l.value.clone());
                                }
                            }
                            LeafOp::Match => {}
                            _ => {
                                if l.value.is_empty() {
                                    l.value = l.values.first().cloned().unwrap_or_default();
                                }
                            }
                        }
                        let options = leaf_op_options(column_class(&specs, &l.column));
                        if !options.iter().any(|o| o == l.op.label()) {
                            l.op = LeafOp::Eq;
                        }
                        st.rules_panel.dirty = true;
                    }
                }
                if let Some(ui) = weak.upgrade() { push(&ui, &s); }
                return;
            }
            let (scope, specs, anchor, old_primary) = {
                let st = s.borrow();
                let scope = st.rules_panel.tab;
                let specs = st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name);
                let anchor = current_anchor(&st);
                let old_primary = selected_validator(&st.rules_panel)
                    .and_then(|v| v.primary_column().map(str::to_string));
                (scope, specs, anchor, old_primary)
            };
            if let Some(key) = CONDITION_KEYS.iter()
                .find(|k| label.as_str() == condition_label(k))
            {
                let specs = {
                    let st = s.borrow();
                    st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name)
                };
                let primary = current_anchor(&s.borrow());
                let node = DraftExpr::Condition(default_condition(
                    key, &specs, primary.as_deref(),
                ));
                {
                    let mut st = s.borrow_mut();
                    let key = st.rules_panel.selected.clone();
                    let replaced = key.map(|key| {
                        let ok = draft_of_mut(&mut st.rules_panel, &key)
                            .and_then(|d| draft_node_mut(d, &key.path))
                            .map(|slot| { *slot = node; })
                            .is_some();
                        ok
                    }).unwrap_or(false);
                    if replaced { st.rules_panel.mark_dirty(); }
                }
                if let Some(ui) = weak.upgrade() { push(&ui, &s); }
                return;
            }
            let key = validator_keys(scope).into_iter()
                .find(|k| label.as_str() == validator_label(k));
            let Some(key) = key else { return };
            let primary = if scope == RuleScope::Column { anchor } else { old_primary };
            let validator = {
                let st = s.borrow();
                default_validator(key, scope, primary.as_deref(), &specs, &st)
            };
            {
                let mut st = s.borrow_mut();
                if let Some(slot) = selected_validator_mut(&mut st.rules_panel) {
                    *slot = validator;
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_inspector_set_primary(move |picked| {
            {
                let mut st = s.borrow_mut();
                if let Some(v) = selected_validator_mut(&mut st.rules_panel) {
                    set_primary_column(v, &picked);
                    if let RuleKind::Compare { other, .. } = v {
                        if picked.as_str() == other.as_str() { *other = String::new(); }
                    }
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_inspector_set_param_a(move |v| {
            let is_condition = {
                let st = s.borrow();
                selected_condition(&st.rules_panel).is_some()
            };
            if is_condition {
                {
                    let mut st = s.borrow_mut();
                    if let Some(l) = selected_condition_mut(&mut st.rules_panel) {
                        match l.op {
                            LeafOp::In => {
                                l.values = split_values(&v);
                                l.value = l.values.first().cloned().unwrap_or_default();
                            }
                            LeafOp::Match => l.pattern = v.to_string(),
                            _ => l.value = v.to_string(),
                        }
                        st.rules_panel.dirty = true;
                    }
                }
                if let Some(ui) = weak.upgrade() { push(&ui, &s); }
                return;
            }
            {
                let mut st = s.borrow_mut();
                let scope = st.rules_panel.tab;
                if let Some(kind) = selected_validator_mut(&mut st.rules_panel) {
                    match kind {
                        RuleKind::Range { min, .. } | RuleKind::Len { min, .. } => *min = v.to_string(),
                        RuleKind::OneOf { values, .. } => *values = split_values(&v),
                        RuleKind::Matches { pattern, .. } => *pattern = v.to_string(),
                        RuleKind::RowCount { min, .. } => *min = v.to_string(),
                        RuleKind::AllNonEmpty { columns } | RuleKind::AtLeastOne { columns }
                        | RuleKind::ExactlyOne { columns } | RuleKind::UniqueCombination { columns }
                            if scope == RuleScope::Table => *columns = split_values(&v),
                        _ => {}
                    }
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_inspector_set_param_b(move |v| {
            {
                let mut st = s.borrow_mut();
                if let Some(kind) = selected_validator_mut(&mut st.rules_panel) {
                    match kind {
                        RuleKind::Range { max, .. } | RuleKind::Len { max, .. } => *max = v.to_string(),
                        RuleKind::RowCount { max, .. } => *max = v.to_string(),
                        _ => {}
                    }
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_inspector_set_param_column(move |picked| {
            let specs = {
                let st = s.borrow();
                st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name)
            };
            let is_condition = {
                let st = s.borrow();
                selected_condition(&st.rules_panel).is_some()
            };
            if is_condition {
                {
                    let mut st = s.borrow_mut();
                    if let Some(l) = selected_condition_mut(&mut st.rules_panel) {
                        l.column = picked.to_string();
                        let options = leaf_op_options(column_class(&specs, &l.column));
                        if !options.iter().any(|o| o == l.op.label()) {
                            l.op = LeafOp::Eq;
                        }
                        st.rules_panel.dirty = true;
                    }
                }
                if let Some(ui) = weak.upgrade() { push(&ui, &s); }
                return;
            }
            {
                let mut st = s.borrow_mut();
                if let Some(RuleKind::Compare { other, .. }) = selected_validator_mut(&mut st.rules_panel) {
                    *other = picked.to_string();
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_inspector_set_param_op(move |picked| {
            let Some(op) = CompareOp::parse(&picked) else { return };
            {
                let mut st = s.borrow_mut();
                if let Some(RuleKind::Compare { op: slot, .. }) = selected_validator_mut(&mut st.rules_panel) {
                    *slot = op;
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_inspector_set_target_node(move |picked| {
            let first_col = target_columns(&s.borrow(), picked.as_str())
                .into_iter().next().unwrap_or_default();
            {
                let mut st = s.borrow_mut();
                if let Some(RuleKind::RefExists { target_node, target_column, .. }) =
                    selected_validator_mut(&mut st.rules_panel)
                {
                    *target_node = picked.to_string();
                    *target_column = first_col;
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_inspector_set_target_column(move |picked| {
            {
                let mut st = s.borrow_mut();
                if let Some(RuleKind::RefExists { target_column, .. }) =
                    selected_validator_mut(&mut st.rules_panel)
                {
                    *target_column = picked.to_string();
                    st.rules_panel.dirty = true;
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }

    // 级别 / 保存 / 还原 / 对话框
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_set_level(move |picked| {
            let level = match picked.as_str() {
                "警告" => Severity::Warning,
                "信息" => Severity::Info,
                _ => Severity::Error,
            };
            {
                let mut st = s.borrow_mut();
                let i = st.rules_panel.selected_rule;
                if i >= 0 {
                    if let Some(d) = st.rules_panel.drafts.get_mut(i as usize) {
                        d.level = level;
                        d.dirty = true;
                        st.rules_panel.dirty = true;
                    }
                }
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_save(move || {
            let (ok, hint) = { let st = s.borrow(); form_status(&st) };
            if !ok {
                s.borrow_mut().engine.ui_log(format!("规则还不能保存：{}", hint));
                if let Some(ui) = weak.upgrade() { push(&ui, &s); }
                return;
            }
            if !commit_drafts(&s) {
                s.borrow_mut().engine.ui_log("保存规则失败，请检查日志。".to_string());
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_revert_rule(move || {
            reset_draft(&s);
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_confirm(move || {
            let (dirty, ok, hint) = {
                let st = s.borrow();
                let (ok, hint) = form_status(&st);
                (st.rules_panel.dirty, ok, hint)
            };
            if dirty && !ok {
                s.borrow_mut().engine.ui_log(format!("规则还不能保存：{}", hint));
                if let Some(ui) = weak.upgrade() { push(&ui, &s); }
                return;
            }
            if dirty && !commit_drafts(&s) { return; }
            let (group, name) = {
                let st = s.borrow();
                (st.rules_panel.group.clone(), st.rules_panel.name.clone())
            };
            {
                let mut st = s.borrow_mut();
                st.engine.revalidate(&group, &name);
                st.rules_panel.open = false;
            }
            if let Some(ui) = weak.upgrade() {
                push(&ui, &s);
                crate::refresh::after_grid_edit(&ui, &s);
            }
        });
    }
    {
        let s = state.clone();
        let weak = ui_h.as_weak();
        ui_h.on_ru_cancel(move || {
            let (group, name, original, undo_len) = {
                let st = s.borrow();
                (st.rules_panel.group.clone(), st.rules_panel.name.clone(),
                 st.rules_panel.session_original.clone(), st.rules_panel.history_undo_len)
            };
            {
                let mut st = s.borrow_mut();
                st.engine.set_node_rules(&group, &name, original);
                if let Some(pid) = st.engine.active_project_id().map(str::to_string) {
                    st.engine.truncate_history(&pid, undo_len);
                }
                st.rules_panel.open = false;
            }
            if let Some(ui) = weak.upgrade() { push(&ui, &s); }
        });
    }
}

fn add_palette_item(state: &Rc<RefCell<AppState>>, ui: &slint::Weak<AppWindow>) {
    let (target, replace, key, scope, specs, primary) = {
        let st = state.borrow();
        let Some(key) = st.rules_panel.palette_selected.clone() else { return };
        let scope = st.rules_panel.tab;
        let specs = st.engine.node_column_classes(&st.rules_panel.group, &st.rules_panel.name);
        let primary = current_anchor(&st);
        (
            st.rules_panel.palette_target.clone(),
            st.rules_panel.palette_replace.clone(),
            key,
            scope,
            specs,
            primary,
        )
    };
    let Some(key_ref) = validator_keys(scope).into_iter().find(|k| *k == key.as_str()) else {
        return;
    };

    if let Some(key) = replace {
        let old_primary = {
            let st = state.borrow();
            st.rules_panel.drafts.get(key.rule)
                .and_then(|d| draft_node(&d.draft, &key.path))
                .and_then(|n| match n {
                    DraftExpr::Validator(v) => v.primary_column().map(str::to_string),
                    _ => None,
                })
        };
        let primary = old_primary.or(primary);
        let validator = {
            let st = state.borrow();
            default_validator(key_ref, scope, primary.as_deref(), &specs, &st)
        };
        let mut st = state.borrow_mut();
        let replaced = draft_of_mut(&mut st.rules_panel, &key)
            .and_then(|d| draft_node_mut(d, &key.path))
            .map(|slot| { *slot = DraftExpr::Validator(validator); })
            .is_some();
        if replaced {
            st.rules_panel.selected_rule = key.rule as i32;
            st.rules_panel.selected = Some(key);
            st.rules_panel.mark_dirty();
            st.rules_panel.palette_open = false;
            st.rules_panel.palette_replace = None;
            st.rules_panel.palette_target = None;
        }
        if let Some(ui) = ui.upgrade() { push(&ui, state); }
        return;
    }

    let Some(key) = target else { return };
    let validator = {
        let st = state.borrow();
        default_validator(key_ref, scope, primary.as_deref(), &specs, &st)
    };
    {
        let mut st = state.borrow_mut();
        st.rules_panel.selected_rule = key.rule as i32;
        let mut new_path = key.path.clone();
        let mut added = false;
        if let Some(draft) = draft_of_mut(&mut st.rules_panel, &key) {
            if let Some(DraftExpr::Group { children, .. }) = draft_node_mut(draft, &key.path) {
                children.push(DraftExpr::Validator(validator));
                new_path.push(children.len() - 1);
                added = true;
            }
        }
        if added {
            st.rules_panel.selected = Some(RowKey { rule: key.rule, path: new_path });
            st.rules_panel.mark_dirty();
            st.rules_panel.palette_open = false;
            st.rules_panel.palette_target = None;
            st.rules_panel.palette_replace = None;
        }
    }
    if let Some(ui) = ui.upgrade() { push(&ui, state); }
}

fn move_tree_node(state: &Rc<RefCell<AppState>>, ui: &slint::Weak<AppWindow>, i: i32, up: bool) {
    let Some(key) = row_key_at(state, i) else { return };
    {
        let mut st = state.borrow_mut();
        st.rules_panel.selected_rule = key.rule as i32;
        let swapped = draft_of_mut(&mut st.rules_panel, &key)
            .map(|d| swap_node(d, &key.path, up))
            .unwrap_or(false);
        if swapped {
            st.rules_panel.mark_dirty();
            st.rules_panel.selected = None;
        }
    }
    if let Some(ui) = ui.upgrade() { push(&ui, state); }
}

fn move_rule(state: &Rc<RefCell<AppState>>, ui: &slint::Weak<AppWindow>, i: i32, up: bool) {
    if i < 0 { return; }
    let idx = i as usize;
    let mut st = state.borrow_mut();
    let len = st.rules_panel.drafts.len();
    if idx >= len { return; }
    let other = if up { idx.checked_sub(1) } else { idx.checked_add(1).filter(|p| *p < len) };
    let Some(other) = other else { return };
    st.rules_panel.drafts.swap(idx, other);
    st.rules_panel.selected_rule = other as i32;
    st.rules_panel.selected = None;
    st.rules_panel.dirty = true;
    drop(st);
    if let Some(ui) = ui.upgrade() { push(&ui, state); }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comma_values_round_trip_quoted() {
        let values = split_values(r#"a, "b,c", "d""e""#);
        assert_eq!(values, vec!["a", "b,c", "d\"e"]);
        assert_eq!(split_values(&join_values(&values)), values);
    }

    #[test]
    fn draft_round_trips_nested_rule_expr() {
        let expr = RuleExpr::And(vec![
            RuleExpr::Validator(RuleKind::NonEmpty { column: "value".into() }),
            RuleExpr::When {
                condition: Condition::Cmp {
                    left: ValueExpr::Column("type".into()),
                    op: CompareOp::Eq,
                    right: ValueExpr::Literal("1".into()),
                },
                inner: Box::new(RuleExpr::Or(vec![
                    RuleExpr::Validator(RuleKind::Matches {
                        column: "value".into(),
                        pattern: "^\\d+$".into(),
                    }),
                    RuleExpr::Validator(RuleKind::Unique { column: "value".into() }),
                ])),
            },
        ]);
        let draft = draft_from_expr(&expr).expect("draft");
        let back = expr_from_draft(&draft).expect("expr");
        assert_eq!(back, expr);
    }

    fn v(c: &str) -> DraftExpr {
        DraftExpr::Validator(RuleKind::NonEmpty { column: c.into() })
    }

    fn group(children: Vec<DraftExpr>, conns: Vec<LogicOp>) -> DraftExpr {
        group_with(children, conns)
    }

    #[test]
    fn simple_mode_is_flat_and_only() {
        assert!(simple_supported(&empty_group()));
        assert!(simple_supported(&group(vec![v("a"), v("b")], vec![LogicOp::And, LogicOp::And])));
        assert!(!simple_supported(&group(vec![v("a"), v("b")], vec![LogicOp::Or, LogicOp::And])));
        assert!(simple_supported(&group(
            vec![group(vec![v("a")], vec![LogicOp::And])],
            vec![LogicOp::And],
        )));
        assert!(!simple_supported(&group(
            vec![group(vec![v("a"), v("b")], vec![LogicOp::Or, LogicOp::And])],
            vec![LogicOp::And],
        )));
        assert!(!simple_supported(&group(
            vec![DraftExpr::Condition(LeafCond {
                column: "type".into(),
                op: LeafOp::Eq,
                value: "1".into(),
                values: vec!["1".into()],
                pattern: String::new(),
            })],
            vec![LogicOp::And],
        )));
    }

    #[test]
    fn normalize_flattens_only_single_child_groups() {
        let draft = group(
            vec![
                group(vec![v("a")], vec![LogicOp::And]),
                group(vec![v("b")], vec![LogicOp::And]),
                group(vec![v("c")], vec![LogicOp::And]),
            ],
            vec![LogicOp::And, LogicOp::And, LogicOp::And],
        );
        let normalized = normalize_draft(draft);
        match normalized {
            DraftExpr::Group { children, conns } => {
                assert_eq!(children.len(), 3);
                assert_eq!(conns.len(), 3);
                assert!(children.iter().all(|c| matches!(c, DraftExpr::Validator(_))));
            }
            other => panic!("unexpected normalized draft: {:?}", other),
        }

        let grouped = group(
            vec![group(vec![v("a"), v("b")], vec![LogicOp::And, LogicOp::And])],
            vec![LogicOp::And],
        );
        match normalize_draft(grouped) {
            DraftExpr::Group { children, .. } => {
                assert!(matches!(&children[0], DraftExpr::Group { children, .. } if children.len() == 2));
            }
            other => panic!("unexpected normalized draft: {:?}", other),
        }
    }

    #[test]
    fn merge_root_wraps_all_children() {
        let mut root = group(vec![v("a"), v("b")], vec![LogicOp::Or, LogicOp::And]);
        assert!(merge_root(&mut root));
        match &root {
            DraftExpr::Group { children, conns } => {
                assert_eq!(children.len(), 1);
                assert_eq!(conns, &vec![LogicOp::And]);
                assert!(matches!(&children[0], DraftExpr::Group { children, .. } if children.len() == 2));
            }
            _ => panic!("root must stay a group"),
        }
        assert!(!merge_root(&mut root), "根层只剩一个组时不能再次合并");
    }

    #[test]
    fn insert_sibling_uses_default_and_connector() {
        let mut root = group(vec![v("a"), v("b")], vec![LogicOp::Or, LogicOp::And]);
        let path = insert_sibling(&mut root, &[0], v("x")).expect("insert");
        assert_eq!(path, vec![1]);
        match &root {
            DraftExpr::Group { children, conns } => {
                assert_eq!(children.len(), 3);
                assert_eq!(conns[0], LogicOp::And, "插入点前一条改成 AND");
                assert_eq!(conns[1], LogicOp::And, "新成员自己的连接符默认 AND");
            }
            _ => panic!("root must stay a group"),
        }
    }

    #[test]
    fn single_member_root_accepts_sibling_insert() {
        // 新建规则必须是「根组 + 一个成员」，否则 +行 / +组 找不到父组。
        let mut root = group_with_one(v("a"));
        let path = insert_sibling(&mut root, &[0], v("b")).expect("insert into single-member root");
        assert_eq!(path, vec![1]);
        match &root {
            DraftExpr::Group { children, .. } => assert_eq!(children.len(), 2),
            _ => panic!("root must stay a group"),
        }
    }

    #[test]
    fn dissolve_group_moves_external_connector_to_last_child() {
        let inner = group(
            vec![v("a"), v("b")],
            vec![LogicOp::Or, LogicOp::Or],
        );
        let mut root = group(vec![inner, v("c")], vec![LogicOp::And, LogicOp::And]);
        let path = dissolve_group(&mut root, &[0]).expect("dissolve");
        assert_eq!(path, vec![0]);
        match &root {
            DraftExpr::Group { children, conns } => {
                assert_eq!(children.len(), 3);
                assert_eq!(conns, &vec![LogicOp::Or, LogicOp::And, LogicOp::And]);
            }
            _ => panic!("root must stay a group"),
        }
    }

    #[test]
    fn prune_removes_groups_emptied_by_delete() {
        let mut root = group(
            vec![
                group(vec![v("a")], vec![LogicOp::And]),
                v("b"),
            ],
            vec![LogicOp::And, LogicOp::And],
        );
        assert!(remove_node(&mut root, &[0, 0]).is_some());
        prune_empty_groups(&mut root);
        match &root {
            DraftExpr::Group { children, conns } => {
                assert_eq!(children.len(), 1);
                assert_eq!(conns.len(), 1);
                assert!(matches!(&children[0], DraftExpr::Validator(RuleKind::NonEmpty { column }) if column == "b"));
            }
            _ => panic!("root must stay a group"),
        }
    }

    #[test]
    fn single_child_groups_are_flattened() {
        let draft = group(
            vec![group(vec![v("x")], vec![LogicOp::Or])],
            vec![LogicOp::And],
        );
        assert_eq!(
            expr_from_draft(&draft),
            Some(RuleExpr::Validator(RuleKind::NonEmpty { column: "x".into() }))
        );
    }

    #[test]
    fn mixed_connectors_compile_with_and_precedence() {
        let draft = group(
            vec![v("a"), v("b"), v("c")],
            vec![LogicOp::Or, LogicOp::And, LogicOp::And],
        );
        let a = || RuleExpr::Validator(RuleKind::NonEmpty { column: "a".into() });
        let b = || RuleExpr::Validator(RuleKind::NonEmpty { column: "b".into() });
        let c = || RuleExpr::Validator(RuleKind::NonEmpty { column: "c".into() });
        assert_eq!(
            expr_from_draft(&draft),
            Some(RuleExpr::Or(vec![
                a(),
                RuleExpr::And(vec![b(), c()]),
            ]))
        );
    }

    #[test]
    fn condition_rows_compile_to_when_filter() {
        let cond = DraftExpr::Condition(LeafCond {
            column: "type".into(),
            op: LeafOp::Eq,
            value: "1".into(),
            values: vec!["1".into()],
            pattern: String::new(),
        });
        let draft = group(
            vec![cond, v("a"), v("b")],
            vec![LogicOp::And, LogicOp::Or, LogicOp::And],
        );
        let expected_cond = Condition::Cmp {
            left: ValueExpr::Column("type".into()),
            op: CompareOp::Eq,
            right: ValueExpr::Literal("1".into()),
        };
        assert_eq!(
            expr_from_draft(&draft),
            Some(RuleExpr::When {
                condition: expected_cond,
                inner: Box::new(RuleExpr::Or(vec![
                    RuleExpr::Validator(RuleKind::NonEmpty { column: "a".into() }),
                    RuleExpr::Validator(RuleKind::NonEmpty { column: "b".into() }),
                ])),
            })
        );
    }
}
