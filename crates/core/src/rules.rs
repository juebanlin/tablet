//! 节点级校验规则：规则类型、级别、内置规则与表达式文本。
//!
//! 这些类型被 model（节点）与 schema_model（section）共同使用；单独成模块，
//! 避免 model / schema_model 互相依赖。

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
    /// 序列化用短码：`error` / `warn`。
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warn",
            Severity::Info => "info",
        }
    }

    /// 严格解析：只认 `error` / `warn`（大小写不敏感）。
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

/// 一条节点级校验规则（@01.10.7）。**列表顺序即优先级**：报告与展示按此顺序。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    /// 本节点的列名：Table 是字段名；Enum 是 `id`/`name`/`desc`。
    pub column: String,
    pub kind: RuleKind,
    pub level: Severity,
    /// 来源。内置规则由 [`builtin_rules`] 生成：不落盘、不可编辑、不可删除。
    pub source: RuleSource,
}

/// 规则来源。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuleSource {
    /// 用户写在 `project.tblmanifest` 里的。
    User,
    /// 系统内置（`builtin_rules`），只读。
    Builtin,
}

impl Rule {
    /// 表达式形态（磁盘与 UI 摘要共用）：`unique()` / `ref_exists(Lang.key)`。
    pub fn expr_string(&self) -> String {
        self.kind.expr_string()
    }

    /// 是否与系统硬编码的校验重复（重复时规则不生效，只报一条非阻断警告）。
    pub fn conflicts_with_builtin(&self, mode: NodeMode) -> bool {
        self.kind.conflicts_with_builtin(mode, &self.column)
    }
}

/// 规则种类。v1 只有两个内置声明式验证器（S25 第 2 层）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleKind {
    /// 本列非空值在节点内不重复。
    Unique,
    /// 本列每个非空值都必须出现在目标节点的目标列里。
    RefExists { target_node: String, target_column: String },
    /// 数值区间 `range(min, max)`；空串表示该侧不限。
    Range { min: String, max: String },
    /// 必填：非空。
    NonEmpty,
    /// 文本长度区间 `len(min, max)`（按字符数）；空串表示该侧不限。
    Len { min: String, max: String },
    /// 值必须是给定集合之一 `one_of(a, b, c)`。
    OneOf { values: Vec<String> },
    /// 文本必须匹配正则 `matches("...")`。
    Matches { pattern: String },
    /// 条件必填：当本行 `column` 的值等于 `value` 时，本列必须有值。
    RequiredIf { column: String, value: String },
    /// 跨列比较：本列与 `column` 按 `op` 比较（如 `min_level <= max_level`）。
    Compare { column: String, op: CompareOp },
    // ── 以下两种只由 builtin_rules 生成，schema 里写不出来 ──
    /// 必须是正整数（> 0）。
    PositiveInt,
    /// 必须是 UPPER_SNAKE_CASE。
    UpperSnake,
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

/// 把参数值写成表达式里的形态：含逗号 / 引号 / 括号或首尾空格时加引号。
fn quote_arg(s: &str) -> String {
    let needs = s.contains(',') || s.contains('"') || s.contains('\\') || s.contains('(')
        || s.contains(')') || s.trim() != s;
    if needs {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

impl RuleKind {
    pub fn expr_string(&self) -> String {
        match self {
            RuleKind::Unique => "unique()".to_string(),
            RuleKind::RefExists { target_node, target_column } =>
                format!("ref_exists({}.{})", target_node, target_column),
            RuleKind::Range { min, max } => format!("range({}, {})", min, max),
            RuleKind::NonEmpty => "non_empty()".to_string(),
            RuleKind::Len { min, max } => format!("len({}, {})", min, max),
            RuleKind::OneOf { values } =>
                format!("one_of({})", values.iter().map(|v| quote_arg(v)).collect::<Vec<_>>().join(", ")),
            RuleKind::Matches { pattern } => format!("matches({})", quote_arg(pattern)),
            RuleKind::RequiredIf { column, value } =>
                format!("required_if({}, {})", column, quote_arg(value)),
            RuleKind::Compare { column, op } =>
                format!("compare({}, {})", column, op.as_str()),
            RuleKind::PositiveInt => "positive_int()".to_string(),
            RuleKind::UpperSnake => "upper_snake()".to_string(),
        }
    }

    /// 这条规则是不是只由系统生成（schema 里不允许出现）。
    pub fn is_builtin_only(&self) -> bool {
        matches!(self, RuleKind::PositiveInt | RuleKind::UpperSnake)
    }

    /// 这条用户规则是否与系统内置规则重复（重复时不执行，只报 `RuleRedundant`）。
    pub fn conflicts_with_builtin(&self, mode: NodeMode, column: &str) -> bool {
        builtin_rules(mode).iter().any(|b| {
            b.column == column && match (&b.kind, self) {
                (RuleKind::Unique, RuleKind::Unique) => true,
                (RuleKind::NonEmpty, RuleKind::NonEmpty) => true,
                // 枚举 id 已保证 > 0：只加下限且下限 ≤ 1 时，整条规则不带来新约束。
                (RuleKind::PositiveInt, RuleKind::Range { min, max }) =>
                    max.trim().is_empty()
                        && min.trim().parse::<f64>().map_or(false, |v| v <= 1.0),
                _ => false,
            }
        })
    }
}

/// 系统内置规则（只读）：由内置校验生成，**不落盘、不可编辑、不可删除**。
///
/// **这是「哪些列已被系统保证」的唯一真相源**：规则面板的只读行、
/// CLI 的有效规则输出、`conflicts_with_builtin` 的冗余判断都读它。
/// 内置校验本身目前仍在 validate.rs 里硬编码（第二步再迁到规则引擎）；
/// 两边必须保持一致。
pub fn builtin_rules(mode: NodeMode) -> Vec<Rule> {
    let mk = |column: &str, kind: RuleKind| Rule {
        column: column.to_string(),
        kind,
        level: Severity::Error,
        source: RuleSource::Builtin,
    };
    match mode {
        NodeMode::Table => vec![
            mk("id", RuleKind::Unique),
            mk("id", RuleKind::NonEmpty),
            mk("id", RuleKind::PositiveInt),
        ],
        NodeMode::Enum => vec![
            mk("id", RuleKind::Unique),
            mk("id", RuleKind::NonEmpty),
            mk("id", RuleKind::PositiveInt),
            mk("name", RuleKind::Unique),
            mk("name", RuleKind::NonEmpty),
            mk("name", RuleKind::UpperSnake),
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

