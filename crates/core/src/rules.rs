//! 节点级校验规则：作用域、表达式、函数目录、级别与内置规则。
//!
//! 设计见 `docs/16-规则表达式与作用域.md`：
//! - `RuleTarget` 决定规则归属（列 / 表）与错误锚点；
//! - `RuleExpr` 是「验证器叶子 + when/and/or」的表达式树；
//! - 函数只有一份签名与一份实现，列级作用域省略主列参数、由锚点补；
//! - `RuleKind` 是叶子验证器，不再包含 `When`。

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Severity {
    /// 阻断保存（现有所有校验码默认都是这一级）。
    #[default]
    Error,
    /// 只提示，不阻断保存；出现在日志与状态栏，不进红框 / 树 `!`。
    Warning,
    /// 信息级：默认不显示（由项目级「验证设置」决定是忽略 / 提示 / 阻断）。
    Info,
}

impl Severity {
    /// 序列化用短码：`error` / `warn` / `info`。
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warn",
            Severity::Info => "info",
        }
    }

    /// 严格解析：只认 `error` / `warn` / `info`（大小写不敏感）。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "error" => Some(Severity::Error),
            "warn" | "warning" => Some(Severity::Warning),
            "info" => Some(Severity::Info),
            _ => None,
        }
    }
}

/// 节点模式。规则去重与列访问都要按模式区分。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeMode {
    Table,
    Constant,
    Enum,
}

/// 规则来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSource {
    /// 用户写在 `project.tblmanifest` 里的。
    User,
    /// 系统内置（`builtin_rules`），只读。
    Builtin,
}

/// 规则作用域：列级 / 表级。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleScope {
    Column,
    Table,
}

/// 求值粒度：逐行 / 全表。它是函数元数据，不是用户可选的作用域。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Granularity {
    Row,
    Table,
}

impl Granularity {
    fn max(self, other: Granularity) -> Granularity {
        match (self, other) {
            (Granularity::Table, _) | (_, Granularity::Table) => Granularity::Table,
            _ => Granularity::Row,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FunctionStatus {
    Active,
    /// 只解析、不序列化；加载时展开成别的表达式。
    Sugar,
    /// 目录里预留，解析时直接报「尚未实现」。
    Planned,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamKind {
    Column,
    Text,
    Regex,
    Bound,
    CompareOp,
    /// `节点.列` 形式的字面量（ref_exists 的目标）。
    NodeColumn,
}

/// 函数目录条目。UI、parser、文档都从这里派生。
#[derive(Debug, Clone, Copy)]
pub struct FunctionSpec {
    pub name: &'static str,
    /// 允许出现的作用域。
    pub scopes: &'static [RuleScope],
    /// 主列参数在 `params` 中的下标；列级作用域省略它。
    pub primary_column: Option<usize>,
    pub granularity: Granularity,
    /// 固定参数。
    pub params: &'static [ParamKind],
    /// 固定参数之后的重复参数（至少一个）。
    pub variadic: Option<ParamKind>,
    pub status: FunctionStatus,
    pub summary: &'static str,
}

/// 规则函数目录（@16.5）。**加函数先改这里，再改 parser/evaluator/UI。**
pub const FUNCTIONS: &[FunctionSpec] = &[
    FunctionSpec {
        name: "unique",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Table,
        params: &[ParamKind::Column],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "非空值在节点内不重复",
    },
    FunctionSpec {
        name: "non_empty",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "必填；整行全空的占位行不算",
    },
    FunctionSpec {
        name: "range",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column, ParamKind::Bound, ParamKind::Bound],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "数值区间；留空一侧不限",
    },
    FunctionSpec {
        name: "len",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column, ParamKind::Bound, ParamKind::Bound],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "字符数区间；留空一侧不限",
    },
    FunctionSpec {
        name: "one_of",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column],
        variadic: Some(ParamKind::Text),
        status: FunctionStatus::Active,
        summary: "值属于候选集合",
    },
    FunctionSpec {
        name: "matches",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column, ParamKind::Regex],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "正则匹配",
    },
    FunctionSpec {
        name: "ref_exists",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column, ParamKind::NodeColumn],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "值必须存在于目标节点.列",
    },
    FunctionSpec {
        name: "compare",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column, ParamKind::CompareOp, ParamKind::Column],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "与另一列比较",
    },
    FunctionSpec {
        name: "positive_int",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "必须是正整数",
    },
    FunctionSpec {
        name: "upper_snake",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "必须 UPPER_SNAKE_CASE",
    },
    FunctionSpec {
        name: "required_if",
        scopes: &[RuleScope::Column, RuleScope::Table],
        primary_column: Some(0),
        granularity: Granularity::Row,
        params: &[ParamKind::Column, ParamKind::Column, ParamKind::Text],
        variadic: None,
        status: FunctionStatus::Sugar,
        summary: "条件必填；展开为 when(eq(条件列, 值), non_empty(col))",
    },
    FunctionSpec {
        name: "all_non_empty",
        scopes: &[RuleScope::Table],
        primary_column: None,
        granularity: Granularity::Row,
        params: &[ParamKind::Column],
        variadic: Some(ParamKind::Column),
        status: FunctionStatus::Active,
        summary: "这些列都必须有值",
    },
    FunctionSpec {
        name: "at_least_one",
        scopes: &[RuleScope::Table],
        primary_column: None,
        granularity: Granularity::Row,
        params: &[ParamKind::Column],
        variadic: Some(ParamKind::Column),
        status: FunctionStatus::Active,
        summary: "至少一列有值",
    },
    FunctionSpec {
        name: "exactly_one",
        scopes: &[RuleScope::Table],
        primary_column: None,
        granularity: Granularity::Row,
        params: &[ParamKind::Column],
        variadic: Some(ParamKind::Column),
        status: FunctionStatus::Active,
        summary: "恰好一列有值",
    },
    FunctionSpec {
        name: "row_count",
        scopes: &[RuleScope::Table],
        primary_column: None,
        granularity: Granularity::Table,
        params: &[ParamKind::Bound, ParamKind::Bound],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "非占位行数在区间内",
    },
    FunctionSpec {
        name: "id_contiguous",
        scopes: &[RuleScope::Table],
        primary_column: None,
        granularity: Granularity::Table,
        params: &[],
        variadic: None,
        status: FunctionStatus::Active,
        summary: "id 集合恰好是 1..=N",
    },
    FunctionSpec {
        name: "unique_combination",
        scopes: &[RuleScope::Table],
        primary_column: None,
        granularity: Granularity::Table,
        params: &[ParamKind::Column],
        variadic: Some(ParamKind::Column),
        status: FunctionStatus::Active,
        summary: "列组合在表内不重复",
    },
    FunctionSpec {
        name: "no_empty_rows",
        scopes: &[RuleScope::Table],
        primary_column: None,
        granularity: Granularity::Table,
        params: &[],
        variadic: None,
        status: FunctionStatus::Planned,
        summary: "不允许整行全空的占位行（第二批）",
    },
];

pub fn function_spec(name: &str) -> Option<&'static FunctionSpec> {
    FUNCTIONS.iter().find(|f| f.name == name)
}

/// 规则在 manifest / 内存里的归属。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleTarget {
    /// 列级：列名同时是错误锚点与省略主列参数的来源。
    Column(String),
    /// 表级：没有锚点列，列参数必须显式。
    Table,
}

impl RuleTarget {
    pub fn column(&self) -> Option<&str> {
        match self {
            RuleTarget::Column(c) => Some(c),
            RuleTarget::Table => None,
        }
    }

    pub fn scope(&self) -> RuleScope {
        match self {
            RuleTarget::Column(_) => RuleScope::Column,
            RuleTarget::Table => RuleScope::Table,
        }
    }
}

/// 规则表达式里的值：列引用或字面量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueExpr {
    Column(String),
    Literal(String),
}

impl ValueExpr {
    pub fn expr_string(&self) -> String {
        match self {
            ValueExpr::Column(name) => name.clone(),
            ValueExpr::Literal(s) => quote_literal(s),
        }
    }

    fn rename_column(&mut self, old: &str, new: &str) {
        if let ValueExpr::Column(name) = self {
            if name == old { *name = new.to_string(); }
        }
    }

    fn columns<'a>(&'a self, out: &mut Vec<&'a str>) {
        if let ValueExpr::Column(name) = self { out.push(name); }
    }
}

/// 行级条件（只出现在 `when` 的第一个参数里）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    /// `eq/ne/lt/le/gt/ge(<值>, <值>)`
    Cmp { left: ValueExpr, op: CompareOp, right: ValueExpr },
    /// `in(<列>, <候选值>...)`
    In { value: ValueExpr, values: Vec<ValueExpr> },
    /// `match(<列>, "<正则>")`
    Match { value: ValueExpr, pattern: String },
    And(Vec<Condition>),
    Or(Vec<Condition>),
    Not(Box<Condition>),
}

impl Condition {
    pub fn expr_string(&self) -> String {
        match self {
            Condition::Cmp { left, op, right } => format!(
                "{}({}, {})", op.fn_name(), left.expr_string(), right.expr_string()
            ),
            Condition::In { value, values } => {
                let mut parts = vec![value.expr_string()];
                parts.extend(values.iter().map(ValueExpr::expr_string));
                format!("in({})", parts.join(", "))
            }
            Condition::Match { value, pattern } =>
                format!("match({}, {})", value.expr_string(), quote_literal(pattern)),
            Condition::And(cs) => format!("and({})", render_conditions(cs)),
            Condition::Or(cs) => format!("or({})", render_conditions(cs)),
            Condition::Not(c) => format!("not({})", c.expr_string()),
        }
    }

    /// 字段改名时同步条件里对该列的引用。
    pub fn rename_column(&mut self, old: &str, new: &str) {
        match self {
            Condition::Cmp { left, right, .. } => {
                left.rename_column(old, new);
                right.rename_column(old, new);
            }
            Condition::In { value, values } => {
                value.rename_column(old, new);
                for v in values { v.rename_column(old, new); }
            }
            Condition::Match { value, .. } => value.rename_column(old, new),
            Condition::And(cs) | Condition::Or(cs) =>
                for c in cs { c.rename_column(old, new); },
            Condition::Not(c) => c.rename_column(old, new),
        }
    }

    pub fn columns<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            Condition::Cmp { left, right, .. } => {
                left.columns(out);
                right.columns(out);
            }
            Condition::In { value, values } => {
                value.columns(out);
                for v in values { v.columns(out); }
            }
            Condition::Match { value, .. } => value.columns(out),
            Condition::And(cs) | Condition::Or(cs) =>
                for c in cs { c.columns(out); },
            Condition::Not(c) => c.columns(out),
        }
    }
}

fn render_conditions(cs: &[Condition]) -> String {
    cs.iter().map(Condition::expr_string).collect::<Vec<_>>().join(", ")
}

/// 验证器叶子。每个函数只有一个变体、一份求值实现。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleKind {
    Unique { column: String },
    NonEmpty { column: String },
    Range { column: String, min: String, max: String },
    Len { column: String, min: String, max: String },
    OneOf { column: String, values: Vec<String> },
    Matches { column: String, pattern: String },
    RefExists { column: String, target_node: String, target_column: String },
    Compare { column: String, op: CompareOp, other: String },
    PositiveInt { column: String },
    UpperSnake { column: String },
    AllNonEmpty { columns: Vec<String> },
    AtLeastOne { columns: Vec<String> },
    ExactlyOne { columns: Vec<String> },
    RowCount { min: String, max: String },
    IdContiguous,
    UniqueCombination { columns: Vec<String> },
    NoEmptyRows,
}

impl RuleKind {
    pub fn name(&self) -> &'static str {
        match self {
            RuleKind::Unique { .. } => "unique",
            RuleKind::NonEmpty { .. } => "non_empty",
            RuleKind::Range { .. } => "range",
            RuleKind::Len { .. } => "len",
            RuleKind::OneOf { .. } => "one_of",
            RuleKind::Matches { .. } => "matches",
            RuleKind::RefExists { .. } => "ref_exists",
            RuleKind::Compare { .. } => "compare",
            RuleKind::PositiveInt { .. } => "positive_int",
            RuleKind::UpperSnake { .. } => "upper_snake",
            RuleKind::AllNonEmpty { .. } => "all_non_empty",
            RuleKind::AtLeastOne { .. } => "at_least_one",
            RuleKind::ExactlyOne { .. } => "exactly_one",
            RuleKind::RowCount { .. } => "row_count",
            RuleKind::IdContiguous => "id_contiguous",
            RuleKind::UniqueCombination { .. } => "unique_combination",
            RuleKind::NoEmptyRows => "no_empty_rows",
        }
    }

    pub fn spec(&self) -> &'static FunctionSpec {
        function_spec(self.name()).expect("function catalog must cover every RuleKind")
    }

    pub fn granularity(&self) -> Granularity {
        self.spec().granularity
    }

    pub fn scope_ok(&self, scope: RuleScope) -> bool {
        self.spec().scopes.contains(&scope)
    }

    /// 该验证器读到的所有列（主列在前）。
    pub fn columns(&self) -> Vec<&str> {
        match self {
            RuleKind::Unique { column }
            | RuleKind::NonEmpty { column }
            | RuleKind::PositiveInt { column }
            | RuleKind::UpperSnake { column } => vec![column],
            RuleKind::Range { column, .. }
            | RuleKind::Len { column, .. }
            | RuleKind::OneOf { column, .. }
            | RuleKind::Matches { column, .. }
            | RuleKind::RefExists { column, .. } => vec![column],
            RuleKind::Compare { column, other, .. } => vec![column, other],
            RuleKind::AllNonEmpty { columns }
            | RuleKind::AtLeastOne { columns }
            | RuleKind::ExactlyOne { columns }
            | RuleKind::UniqueCombination { columns } =>
                columns.iter().map(String::as_str).collect(),
            RuleKind::RowCount { .. } | RuleKind::IdContiguous | RuleKind::NoEmptyRows => Vec::new(),
        }
    }

    /// 主列参数（列级作用域里省略的那个）。
    pub fn primary_column(&self) -> Option<&str> {
        match self {
            RuleKind::Unique { column }
            | RuleKind::NonEmpty { column }
            | RuleKind::Range { column, .. }
            | RuleKind::Len { column, .. }
            | RuleKind::OneOf { column, .. }
            | RuleKind::Matches { column, .. }
            | RuleKind::RefExists { column, .. }
            | RuleKind::Compare { column, .. }
            | RuleKind::PositiveInt { column }
            | RuleKind::UpperSnake { column } => Some(column),
            _ => None,
        }
    }

    pub fn rename_column(&mut self, old: &str, new: &str) {
        let rename = |s: &mut String| {
            if s == old { *s = new.to_string(); }
        };
        match self {
            RuleKind::Unique { column }
            | RuleKind::NonEmpty { column }
            | RuleKind::Range { column, .. }
            | RuleKind::Len { column, .. }
            | RuleKind::OneOf { column, .. }
            | RuleKind::Matches { column, .. }
            | RuleKind::RefExists { column, .. }
            | RuleKind::PositiveInt { column }
            | RuleKind::UpperSnake { column } => rename(column),
            RuleKind::Compare { column, other, .. } => {
                rename(column);
                rename(other);
            }
            RuleKind::AllNonEmpty { columns }
            | RuleKind::AtLeastOne { columns }
            | RuleKind::ExactlyOne { columns }
            | RuleKind::UniqueCombination { columns } =>
                for c in columns { rename(c); },
            RuleKind::RowCount { .. } | RuleKind::IdContiguous | RuleKind::NoEmptyRows => {}
        }
    }

    pub fn references_column(&self, name: &str) -> bool {
        self.columns().iter().any(|c| *c == name)
    }

    /// 表达式文本。`omit_primary` 只在「列级 + 单个验证器」时为 true。
    pub fn expr_string(&self, omit_primary: bool) -> String {
        let mut args: Vec<String> = match self {
            RuleKind::Unique { column }
            | RuleKind::NonEmpty { column }
            | RuleKind::PositiveInt { column }
            | RuleKind::UpperSnake { column } => vec![column.clone()],
            RuleKind::Range { column, min, max }
            | RuleKind::Len { column, min, max } =>
                vec![column.clone(), min.clone(), max.clone()],
            RuleKind::OneOf { column, values } => {
                let mut out = vec![column.clone()];
                out.extend(values.iter().map(|v| quote_literal(v)));
                out
            }
            RuleKind::Matches { column, pattern } =>
                vec![column.clone(), quote_literal(pattern)],
            RuleKind::RefExists { column, target_node, target_column } =>
                vec![column.clone(), format!("{}.{}", target_node, target_column)],
            RuleKind::Compare { column, op, other } =>
                vec![column.clone(), op.as_str().to_string(), other.clone()],
            RuleKind::AllNonEmpty { columns }
            | RuleKind::AtLeastOne { columns }
            | RuleKind::ExactlyOne { columns }
            | RuleKind::UniqueCombination { columns } => columns.clone(),
            RuleKind::RowCount { min, max } => vec![min.clone(), max.clone()],
            RuleKind::IdContiguous | RuleKind::NoEmptyRows => Vec::new(),
        };
        let spec = self.spec();
        if omit_primary {
            if let Some(idx) = spec.primary_column {
                if idx < args.len() { args.remove(idx); }
            }
        }
        format!("{}({})", spec.name, args.join(", "))
    }
}

/// 规则表达式（L4）：验证器叶子 + when/and/or。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleExpr {
    Validator(RuleKind),
    When { condition: Condition, inner: Box<RuleExpr> },
    And(Vec<RuleExpr>),
    Or(Vec<RuleExpr>),
}

impl RuleExpr {
    pub fn granularity(&self) -> Granularity {
        match self {
            RuleExpr::Validator(v) => v.granularity(),
            RuleExpr::When { inner, .. } => inner.granularity(),
            RuleExpr::And(cs) | RuleExpr::Or(cs) =>
                cs.iter().map(RuleExpr::granularity)
                    .fold(Granularity::Row, Granularity::max),
        }
    }

    pub fn has_table_granularity(&self) -> bool {
        self.granularity() == Granularity::Table
    }

    pub fn is_single_validator(&self) -> bool {
        matches!(self, RuleExpr::Validator(_))
    }

    pub fn columns(&self) -> Vec<&str> {
        let mut out = Vec::new();
        self.collect_columns(&mut out);
        out
    }

    fn collect_columns<'a>(&'a self, out: &mut Vec<&'a str>) {
        match self {
            RuleExpr::Validator(v) => out.extend(v.columns()),
            RuleExpr::When { condition, inner } => {
                condition.columns(out);
                inner.collect_columns(out);
            }
            RuleExpr::And(cs) | RuleExpr::Or(cs) =>
                for c in cs { c.collect_columns(out); },
        }
    }

    pub fn rename_column(&mut self, old: &str, new: &str) {
        match self {
            RuleExpr::Validator(v) => v.rename_column(old, new),
            RuleExpr::When { condition, inner } => {
                condition.rename_column(old, new);
                inner.rename_column(old, new);
            }
            RuleExpr::And(cs) | RuleExpr::Or(cs) =>
                for c in cs { c.rename_column(old, new); },
        }
    }

    pub fn for_each_validator(&self, f: &mut impl FnMut(&RuleKind)) {
        match self {
            RuleExpr::Validator(v) => f(v),
            RuleExpr::When { inner, .. } => inner.for_each_validator(f),
            RuleExpr::And(cs) | RuleExpr::Or(cs) =>
                for c in cs { c.for_each_validator(f); },
        }
    }

    pub fn for_each_validator_mut(&mut self, f: &mut impl FnMut(&mut RuleKind)) {
        match self {
            RuleExpr::Validator(v) => f(v),
            RuleExpr::When { inner, .. } => inner.for_each_validator_mut(f),
            RuleExpr::And(cs) | RuleExpr::Or(cs) =>
                for c in cs { c.for_each_validator_mut(f); },
        }
    }

    /// `omit_primary` 只对单个验证器生效；组合里所有列都显式。
    pub fn expr_string(&self, omit_primary: bool) -> String {
        match self {
            RuleExpr::Validator(v) => v.expr_string(omit_primary),
            RuleExpr::When { condition, inner } =>
                format!("when({}, {})", condition.expr_string(), inner.expr_string(omit_primary)),
            RuleExpr::And(cs) =>
                format!("and({})", cs.iter().map(|c| c.expr_string(omit_primary)).collect::<Vec<_>>().join(", ")),
            RuleExpr::Or(cs) =>
                format!("or({})", cs.iter().map(|c| c.expr_string(omit_primary)).collect::<Vec<_>>().join(", ")),
        }
    }
}

/// 一条节点级校验规则。**列表顺序即优先级**：报告与展示按此顺序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub target: RuleTarget,
    pub expr: RuleExpr,
    pub level: Severity,
    /// 来源。内置规则由 [`builtin_rules`] 生成：不落盘、不可编辑、不可删除。
    pub source: RuleSource,
}

impl Rule {
    pub fn column_rule(column: &str, kind: RuleKind, level: Severity, source: RuleSource) -> Self {
        Self {
            target: RuleTarget::Column(column.to_string()),
            expr: RuleExpr::Validator(kind),
            level,
            source,
        }
    }

    pub fn table_rule(expr: RuleExpr, level: Severity, source: RuleSource) -> Self {
        Self { target: RuleTarget::Table, expr, level, source }
    }

    pub fn target_column(&self) -> Option<&str> {
        self.target.column()
    }

    pub fn scope(&self) -> RuleScope {
        self.target.scope()
    }

    pub fn granularity(&self) -> Granularity {
        self.expr.granularity()
    }

    /// 磁盘 / UI 摘要：`column value: non_empty()` / `table: all_non_empty(...)`。
    pub fn expr_string(&self) -> String {
        self.body_string()
    }

    pub fn body_string(&self) -> String {
        match &self.target {
            RuleTarget::Column(col) => format!(
                "column {}: {}", col,
                self.expr.expr_string(true)
            ),
            RuleTarget::Table => format!("table: {}", self.expr.expr_string(false)),
        }
    }

    pub fn rename_column(&mut self, old: &str, new: &str) {
        if let RuleTarget::Column(col) = &mut self.target {
            if col == old { *col = new.to_string(); }
        }
        self.expr.rename_column(old, new);
    }

    /// 是否与系统内置规则完全等价（重复时规则不生效，只报一条非阻断警告）。
    pub fn conflicts_with_builtin(&self, mode: NodeMode) -> bool {
        builtin_rules(mode).iter().any(|b| {
            b.target == self.target && b.expr == self.expr
        })
    }
}

/// 跨列比较运算符。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
}

impl CompareOp {
    pub fn as_str(self) -> &'static str {
        match self {
            CompareOp::Lt => "<",
            CompareOp::Le => "<=",
            CompareOp::Gt => ">",
            CompareOp::Ge => ">=",
            CompareOp::Eq => "==",
            CompareOp::Ne => "!=",
        }
    }

    pub fn fn_name(self) -> &'static str {
        match self {
            CompareOp::Lt => "lt",
            CompareOp::Le => "le",
            CompareOp::Gt => "gt",
            CompareOp::Ge => "ge",
            CompareOp::Eq => "eq",
            CompareOp::Ne => "ne",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s.trim() {
            "<" => Some(CompareOp::Lt),
            "<=" => Some(CompareOp::Le),
            ">" => Some(CompareOp::Gt),
            ">=" => Some(CompareOp::Ge),
            "==" | "=" => Some(CompareOp::Eq),
            "!=" | "<>" => Some(CompareOp::Ne),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CompareOp::Lt => "小于",
            CompareOp::Le => "小于等于",
            CompareOp::Gt => "大于",
            CompareOp::Ge => "大于等于",
            CompareOp::Eq => "等于",
            CompareOp::Ne => "不等于",
        }
    }
}

/// 能不加引号直接写的字面量：数字 / true / false。
fn is_bare_literal(s: &str) -> bool {
    if s == "true" || s == "false" { return true; }
    let digits_ok = s.chars().all(|c| {
        c.is_ascii_digit() || matches!(c, '.' | '+' | '-' | 'e' | 'E')
    });
    digits_ok && s.parse::<f64>().is_ok()
}

pub fn quote_literal(s: &str) -> String {
    if is_bare_literal(s) { return s.to_string(); }
    format!("\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\t', "\\t"))
}

/// 系统内置规则（只读）：由内置校验生成，**不落盘、不可编辑、不可删除**。
///
/// **这是「哪些列已被系统保证」的唯一真相源**：规则面板的只读行、
/// CLI 的有效规则输出、`conflicts_with_builtin` 的冗余判断都读它。
pub fn builtin_rules(mode: NodeMode) -> Vec<Rule> {
    let mk = |column: &str, kind: RuleKind| Rule::column_rule(
        column, kind, Severity::Error, RuleSource::Builtin,
    );
    match mode {
        NodeMode::Table => vec![
            mk("id", RuleKind::Unique { column: "id".into() }),
            mk("id", RuleKind::NonEmpty { column: "id".into() }),
            mk("id", RuleKind::PositiveInt { column: "id".into() }),
        ],
        NodeMode::Enum => vec![
            mk("id", RuleKind::Unique { column: "id".into() }),
            mk("id", RuleKind::NonEmpty { column: "id".into() }),
            mk("id", RuleKind::PositiveInt { column: "id".into() }),
            mk("name", RuleKind::Unique { column: "name".into() }),
            mk("name", RuleKind::NonEmpty { column: "name".into() }),
            mk("name", RuleKind::UpperSnake { column: "name".into() }),
        ],
        NodeMode::Constant => Vec::new(),
    }
}

/// 有效规则 = 系统内置（只读，排在前）+ 用户规则。
pub fn effective_rules(mode: NodeMode, user: &[Rule]) -> Vec<Rule> {
    let mut out = builtin_rules(mode);
    out.extend(user.iter().cloned());
    out
}
