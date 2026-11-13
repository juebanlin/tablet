use crate::model::*;
use crate::types::{BaseType, Paradigm, SeparatorsSection, TblType, TypeParam};

// ────────────────────────────────────────────────────────────────────────────
// 验证结构（cell / row / schema / table 各层共用）
// ────────────────────────────────────────────────────────────────────────────

/// 表头层错误使用的行号哨兵值，区分于普通数据行。
pub const SCHEMA_ROW: usize = usize::MAX;
/// 表级错误（规则作用于整张表，无法定位到具体数据行）。
pub const TABLE_ROW: usize = usize::MAX - 1;

/// Table 表头四行（按 UI/编辑器从上到下顺序，1-based "第 N 行"）。
/// 与 .tbl 文件序列化顺序无关，仅决定日志/UI 的展示行号。
/// 用作数据索引时必须调用 [`TableHeaderRow::row`]（返回 0-based）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum TableHeaderRow {
    /// 第 1 行：字段描述
    Desc = 1,
    /// 第 2 行:导出范围（client/server/...）
    Export = 2,
    /// 第 3 行：字段类型
    Type = 3,
    /// 第 4 行：字段名
    Field = 4,
}

impl TableHeaderRow {
    /// 0-based 行索引（用于 `header_rows[i]` 之类的下标）。
    pub fn row(self) -> usize { (self as usize) - 1 }
}

/// Constant 行内列号枚举：每行 5 列固定范式（1-based "第 N 列"）。
/// 用作列索引时必须调用 [`ConstantCol::col`]（返回 0-based）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum ConstantCol {
    /// 第 1 列：常量名
    Name = 1,
    /// 第 2 列：类型
    Type = 2,
    /// 第 3 列：常量值
    Value = 3,
    /// 第 4 列：导出范围
    Export = 4,
    /// 第 5 列：描述
    Desc = 5,
}

impl ConstantCol {
    /// 0-based 列索引（用于 records 下标）。
    pub fn col(self) -> usize { (self as usize) - 1 }
}

/// Enum 行内列号枚举：每行 3 列固定范式（1-based "第 N 列"）。
/// 用作列索引时必须调用 [`EnumCol::col`]（返回 0-based）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum EnumCol {
    /// 第 1 列：枚举 id
    Id = 1,
    /// 第 2 列：枚举条目名
    Name = 2,
    /// 第 3 列：描述
    Desc = 3,
}

impl EnumCol {
    /// 0-based 列索引（用于 entries 字段访问）。
    pub fn col(self) -> usize { (self as usize) - 1 }
}

/// 验证错误分类码（HTTP response 风格的"业务码"）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ValidationCode {
    // schema
    SchemaEmptyFields,
    SchemaIdNotFirst,
    FieldNameInvalid,
    FieldNameKeyword,
    FieldNameDuplicate,
    /// 项目内节点名重复（Table / Constant / Enum 共用平铺命名空间）。
    NodeNameDuplicate,
    /// 转换后键名撞车（表字段 / 常量条目共用）：`stack_` 与 `stack` 转出来都是 `stack`，
    /// 生成到 Java/Go/C++/C#/TS 时会变成两个同名成员。规则见 @01.8.1。
    NameCollision,
    TypeInvalid,
    TypeRefMissing,
    TypeRefIsConstant,
    ConstantRefForbidden,
    EnumEmpty,
    // row / cell
    BaseTypeMismatch,
    IdNotInteger,
    NameNotIdentifier,
    RefValueMissing,
    NameFilledButValueEmpty,
    /// 常量名重复（常量不参与节点规则，仍由内置检查负责）。
    DuplicateName,
    // 节点级规则（@01.10.7）
    /// `unique()` 命中重复值。
    RuleUniqueViolation,
    /// `ref_exists()` 的值在目标列里找不到。
    RuleRefValueMissing,
    /// `ref_exists()` 的目标节点不存在（或已被标删）。
    RuleTargetMissing,
    /// `ref_exists()` 的目标列不存在。
    RuleTargetColumnMissing,
    /// `ref_exists()` 的目标节点在项目里重名，无法确定引用哪一张。
    RuleTargetAmbiguous,
    /// 规则指向的本节点列不存在。
    RuleColumnMissing,
    /// 规则与系统硬编码的校验重复（如对 enum 的 `name` 再挂 `unique()`）。
    /// 规则不生效，只报这一条非阻断提示。
    RuleRedundant,
    /// 规则挂在了不支持的类型上（复合并列表，或目标列是复合类型）。
    RuleTypeIncompatible,
    /// 规则参数不合法（区间两端颠倒 / 正则编译失败 / 空集合 …）。
    RuleParamInvalid,
    RuleRangeViolation,
    RuleNotEmptyViolation,
    RuleLenViolation,
    RuleOneOfViolation,
    RulePatternViolation,
    RuleRequiredIfMissing,
    RuleCompareViolation,
    RulePositiveIntViolation,
    RuleUpperSnakeViolation,
    /// 行级多列函数（all_non_empty / at_least_one / exactly_one）失败。
    RuleRowViolation,
    /// 全表函数（row_count / id_contiguous / unique_combination）失败。
    RuleTableViolation,
    Other,
}

/// 一条验证错误（atomic）：含错误码、定位信息、原值与人类可读消息。
/// 表示"无错误"用 `Option::None` 或空 `Vec`，不在本结构内体现。
#[derive(Clone, Debug)]
pub struct ValidationError {
    pub code: ValidationCode,
    /// 级别（@01.10.7）：Error 阻断保存；Warning 只提示。现有校验码一律默认 Error。
    pub severity: Severity,
    /// SCHEMA_ROW 表示表头层错误，其它为 0-based 数据行索引。
    pub row: usize,
    pub col: usize,
    /// 仅 Table schema 层有意义；Constant/Enum schema 与数据行均为 None。
    pub header_row: Option<TableHeaderRow>,
    pub field: String,
    pub value: String,
    pub message: String,
    /// 这条错误由节点的第几条规则产生（`# @rule` 的行序）；非规则错误为 None。
    pub rule_index: Option<usize>,
    /// 行级/表级规则涉及的列（列级规则为空或单列）。
    pub affected: Vec<String>,
}

impl ValidationError {
    pub fn is_schema(&self) -> bool { self.row == SCHEMA_ROW }

    pub fn is_table_level(&self) -> bool { self.row == TABLE_ROW }

    /// cell 层用：仅有 code + message，行/列/字段由调用方在 row 层填充。
    pub(crate) fn cell(code: ValidationCode, message: String) -> Self {
        Self {
            code, severity: Severity::Error, row: 0, col: 0,
            header_row: None,
            field: String::new(),
            value: String::new(),
            message,
            rule_index: None,
            affected: Vec::new(),
        }
    }

    /// 把级别改成规则声明的级别（`# @rule ... [warn]`）。
    pub fn with_level(mut self, level: Severity) -> Self {
        self.severity = level;
        self
    }

    pub fn is_warning(&self) -> bool { self.severity == Severity::Warning }

    pub fn is_info(&self) -> bool { self.severity == Severity::Info }

    /// 标记这条错误来自节点的第几条规则。
    pub(crate) fn with_rule_index(mut self, idx: usize) -> Self {
        self.rule_index = Some(idx);
        self
    }

    pub(crate) fn with_pos(mut self, row: usize, col: usize) -> Self {
        self.row = row; self.col = col; self
    }

    pub(crate) fn with_row(mut self, row: usize) -> Self {
        self.row = row; self
    }

    pub(crate) fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = field.into(); self
    }

    pub(crate) fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into(); self
    }

    pub(crate) fn with_affected(mut self, affected: Vec<String>) -> Self {
        self.affected = affected;
        self
    }

    /// Constant/Enum cell 层常用：一次填好 col + field + value（row 由 row 层补）。
    fn with_col_field_value(mut self, col: usize, field: impl Into<String>, value: impl Into<String>) -> Self {
        self.col = col;
        self.field = field.into();
        self.value = value.into();
        self
    }

    /// schema 层常用：标记为表头错误（row=SCHEMA_ROW），可选指定 Table 表头行号。
    pub(crate) fn schema(code: ValidationCode, message: String, col: usize, header_row: Option<TableHeaderRow>) -> Self {
        Self {
            code,
            severity: Severity::Error,
            row: SCHEMA_ROW,
            col,
            header_row,
            field: String::new(),
            value: String::new(),
            message,
            rule_index: None,
            affected: Vec::new(),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 引用索引与基础校验
// ────────────────────────────────────────────────────────────────────────────

/// 引用目标类型
#[derive(Debug, Clone, PartialEq)]
pub enum RefKind {
    Table,
    Enum,
    Constant,
}

/// 项目级引用索引：name → (kind, 有效 id 集)
#[derive(Debug, Default)]
pub struct RefIndex {
    /// name → (kind, set<id 字符串>)
    map: std::collections::HashMap<String, (RefKind, std::collections::HashSet<String>)>,
}

impl RefIndex {
    pub fn build(groups: &[Group]) -> Self {
        let mut map: std::collections::HashMap<String, (RefKind, std::collections::HashSet<String>)>
            = std::collections::HashMap::new();
        for g in groups {
            for t in &g.tables {
                if t.deleted { continue; }
                let idx = t.schema.fields.iter().position(|f| !f.is_aux() && f.name == "id");
                let mut ids = std::collections::HashSet::new();
                if let Some(idx) = idx {
                    for row in &t.records {
                        let v = row.get(idx).map(|s| s.as_str()).unwrap_or("");
                        if !v.is_empty() { ids.insert(v.to_string()); }
                    }
                }
                map.insert(t.name.clone(), (RefKind::Table, ids));
            }
            for e in &g.enums {
                if e.deleted { continue; }
                let ids: std::collections::HashSet<String> = e.entries.iter()
                    .filter(|en| !en.id.is_empty())
                    .map(|en| en.id.clone())
                    .collect();
                map.insert(e.name.clone(), (RefKind::Enum, ids));
            }
            for c in &g.constants {
                if c.deleted { continue; }
                map.insert(c.name.clone(), (RefKind::Constant, std::collections::HashSet::new()));
            }
        }
        Self { map }
    }

    pub fn lookup(&self, name: &str) -> Option<&RefKind> {
        self.map.get(name).map(|(k, _)| k)
    }

    pub fn id_exists(&self, name: &str, id: &str) -> bool {
        self.map.get(name).map(|(_, set)| set.contains(id)).unwrap_or(false)
    }
}

// ────────────────────────────────────────────────────────────────────────────
// 节点级规则索引（@01.10.7）
// ────────────────────────────────────────────────────────────────────────────

/// Enum 的可用列。
pub const ENUM_RULE_COLUMNS: &[&str] = &["id", "name", "desc"];

/// 规则可用的值类（@01.10.7）：决定一列能不能挂规则，以及值怎么比。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueClass {
    /// 文本 / 布尔 / 引用 id：按 trim 后原字符串比较。
    Text,
    /// int / long：按整数比较（`01` 与 `1` 相同）。
    Int,
    /// float：按浮点比较（`1.0` 与 `1` 相同）。
    Float,
}

impl ValueClass {
    /// 比较用的规范化形式。整数列解析失败时退回原文，让校验报出真实差异。
    pub fn normalize(self, raw: &str) -> String {
        let t = raw.trim();
        match self {
            ValueClass::Int => t.parse::<i64>()
                .map(|n| n.to_string())
                .unwrap_or_else(|_| t.to_string()),
            ValueClass::Float => t.parse::<f64>()
                .map(|n| n.to_string())
                .unwrap_or_else(|_| t.to_string()),
            ValueClass::Text => t.to_string(),
        }
    }
}

/// 由字段类型串得出值类；复合并列表返回 None（v1 不参与规则）。
pub fn value_class_of_type(tbl_type: &str) -> Option<ValueClass> {
    let t = TblType::parse(tbl_type)?;
    match t.paradigm {
        Paradigm::Ref => Some(ValueClass::Text),
        Paradigm::Base => match t.params.first() {
            Some(TypeParam::Base(b)) if matches!(b, BaseType::Int | BaseType::Long | BaseType::Date) =>
                Some(ValueClass::Int),
            Some(TypeParam::Base(BaseType::Float)) => Some(ValueClass::Float),
            Some(TypeParam::Base(_)) => Some(ValueClass::Text),
            _ => None,
        },
        _ => None,
    }
}

/// 一列在规则里的规格：(列名, 值类)。值类 `None` = 该列类型不支持规则。
pub type ColumnSpec = (String, Option<ValueClass>);

/// Table 的列规格（按字段声明顺序）。
pub fn table_column_specs(table: &Table) -> Vec<ColumnSpec> {
    table.schema.fields.iter()
        .filter(|f| !f.is_aux())
        .map(|f| (f.name.clone(), value_class_of_type(&f.tbl_type)))
        .collect()
}

/// Enum 的列规格：`id` 是整数，其余是文本。
pub fn enum_column_specs() -> Vec<ColumnSpec> {
    ENUM_RULE_COLUMNS.iter()
        .map(|c| {
            let class = if *c == "id" { ValueClass::Int } else { ValueClass::Text };
            ((*c).to_string(), Some(class))
        })
        .collect()
}

/// 节点级规则用的索引。
///
/// 回答两类问题：目标节点 / 目标列是否存在、目标列是什么值类（schema 层，不读行），
/// 以及某个「被 `ref_exists` 指向的列」有哪些非空值（已按值类规范化）。
/// **只扫描规则真正引用到的目标列**，避免全项目全列扫描。
pub struct RuleIndex {
    /// 节点名 → 列规格（保持字段声明顺序，UI 下拉直接按此顺序展示）
    columns: std::collections::HashMap<String, Vec<ColumnSpec>>,
    /// 项目内重名的节点名（引用它时无法确定目标）
    ambiguous: std::collections::HashSet<String>,
    /// (节点名, 列名) → (值类, 规范化后的非空值集合)
    values: std::collections::HashMap<(String, String), (ValueClass, std::collections::HashSet<String>)>,
}

/// 三种节点在「列访问」上的统一视图。
enum NodeRef<'a> {
    Table(&'a Table),
    Constant(&'a Constant),
    Enum(&'a EnumDef),
}

impl<'a> NodeRef<'a> {
    /// 项目内全部未删除节点。
    fn all(groups: &'a [Group]) -> Vec<NodeRef<'a>> {
        let mut out = Vec::new();
        for g in groups {
            for t in &g.tables { if !t.deleted { out.push(NodeRef::Table(t)); } }
            for c in &g.constants { if !c.deleted { out.push(NodeRef::Constant(c)); } }
            for e in &g.enums { if !e.deleted { out.push(NodeRef::Enum(e)); } }
        }
        out
    }

    fn name(&self) -> &str {
        match self {
            NodeRef::Table(t) => &t.name,
            NodeRef::Constant(c) => &c.name,
            NodeRef::Enum(e) => &e.name,
        }
    }

    fn rules(&self) -> &[Rule] {
        match self {
            NodeRef::Table(t) => &t.rules,
            // 常量不支持规则（@01.8.8）
            NodeRef::Constant(_) => &[],
            NodeRef::Enum(e) => &e.rules,
        }
    }

    fn column_specs(&self) -> Vec<ColumnSpec> {
        match self {
            NodeRef::Table(t) => table_column_specs(t),
            // 常量不支持规则（@01.8.8）
            NodeRef::Constant(_) => Vec::new(),
            NodeRef::Enum(_) => enum_column_specs(),
        }
    }

    fn column_class(&self, column: &str) -> Option<ValueClass> {
        self.column_specs().into_iter()
            .find(|(c, _)| c == column)
            .and_then(|(_, cls)| cls)
    }

    fn row_count(&self) -> usize {
        match self {
            NodeRef::Table(t) => t.records.len(),
            NodeRef::Constant(c) => c.entries.len(),
            NodeRef::Enum(e) => e.entries.len(),
        }
    }

    /// 取 (行, 列) 的原始值；列名不存在返回 None。
    fn cell(&self, row: usize, column: &str) -> Option<String> {
        match self {
            NodeRef::Table(t) => {
                let idx = t.schema.fields.iter().position(|f| !f.is_aux() && f.name == column)?;
                t.records.get(row)?.get(idx).cloned()
            }
            NodeRef::Constant(c) => {
                let e = c.entries.get(row)?;
                Some(match column {
                    "name" => e.name.clone(),
                    "type" => e.tbl_type.clone(),
                    "value" => e.value.clone(),
                    "export" => e.export.code().to_string(),
                    "desc" => e.desc.clone(),
                    _ => return None,
                })
            }
            NodeRef::Enum(e) => {
                let en = e.entries.get(row)?;
                Some(match column {
                    "id" => en.id.clone(),
                    "name" => en.name.clone(),
                    "desc" => en.desc.clone(),
                    _ => return None,
                })
            }
        }
    }
}

fn collect_ref_targets(
    expr: &RuleExpr,
    out: &mut std::collections::HashSet<(String, String)>,
) {
    match expr {
        RuleExpr::Validator(RuleKind::RefExists { target_node, target_column, .. }) => {
            out.insert((target_node.clone(), target_column.clone()));
        }
        RuleExpr::Validator(_) => {}
        RuleExpr::When { inner, .. } => collect_ref_targets(inner, out),
        RuleExpr::And(cs) | RuleExpr::Or(cs) =>
            for c in cs { collect_ref_targets(c, out); },
    }
}

impl RuleIndex {
    /// 只建 schema 层信息（节点 / 列存在性 + 值类），不读任何数据行。
    pub fn build_schema(groups: &[Group]) -> Self {
        let mut columns = std::collections::HashMap::new();
        let mut ambiguous = std::collections::HashSet::new();
        let mut seen = std::collections::HashSet::new();
        for node in NodeRef::all(groups) {
            let name = node.name().to_string();
            if !seen.insert(name.clone()) {
                ambiguous.insert(name.clone());
            }
            columns.insert(name, node.column_specs());
        }
        Self { columns, ambiguous, values: std::collections::HashMap::new() }
    }

    /// schema + 只扫描被 `ref_exists` 指向的目标列。
    pub fn build(groups: &[Group]) -> Self {
        let mut idx = Self::build_schema(groups);
        let mut wanted: std::collections::HashSet<(String, String)> = std::collections::HashSet::new();
        for node in NodeRef::all(groups) {
            for rule in node.rules() {
                collect_ref_targets(&rule.expr, &mut wanted);
            }
        }
        if wanted.is_empty() {
            return idx;
        }
        for node in NodeRef::all(groups) {
            let name = node.name().to_string();
            let cols: Vec<String> = wanted.iter()
                .filter(|(n, _)| n == &name)
                .map(|(_, c)| c.clone())
                .collect();
            for col in cols {
                // 复合类型列不参与规则，直接跳过（校验层会报 RuleTypeIncompatible）。
                let Some(class) = node.column_class(&col) else { continue };
                let mut set = std::collections::HashSet::new();
                for r in 0..node.row_count() {
                    if let Some(v) = node.cell(r, &col) {
                        let nv = class.normalize(&v);
                        if !nv.is_empty() { set.insert(nv); }
                    }
                }
                idx.values.insert((name.clone(), col), (class, set));
            }
        }
        idx
    }

    pub fn node_exists(&self, name: &str) -> bool { self.columns.contains_key(name) }

    pub fn column_exists(&self, name: &str, column: &str) -> bool {
        self.columns.get(name).map_or(false, |c| c.iter().any(|(x, _)| x == column))
    }

    pub fn is_ambiguous(&self, name: &str) -> bool { self.ambiguous.contains(name) }

    /// 目标列的规范化值集与值类。未被任何规则引用 / 列类型不支持时返回 None。
    pub fn values(&self, name: &str, column: &str)
        -> Option<(ValueClass, &std::collections::HashSet<String>)>
    {
        self.values.get(&(name.to_string(), column.to_string()))
            .map(|(class, set)| (*class, set))
    }

    /// 某列的值类；列不存在或类型不支持规则时返回 None。
    pub fn column_class(&self, name: &str, column: &str) -> Option<ValueClass> {
        self.columns.get(name)?
            .iter()
            .find(|(c, _)| c == column)?
            .1
    }

    /// 节点可用的列规格（给 UI 下拉用，顺序与字段声明一致）。节点不存在返回空。
    pub fn column_specs_of(&self, name: &str) -> Vec<ColumnSpec> {
        self.columns.get(name).cloned().unwrap_or_default()
    }

    /// 节点可用的列名。节点不存在返回空。
    pub fn columns_of(&self, name: &str) -> Vec<String> {
        self.columns.get(name)
            .map(|v| v.iter().map(|(c, _)| c.clone()).collect())
            .unwrap_or_default()
    }
}

/// 项目级节点名唯一性：Table / Constant / Enum 共用同一个平铺命名空间。
/// 返回每个重复节点的 `(group, node, error)`；调用方决定按项目策略过滤。
pub fn validate_project_node_names(groups: &[Group]) -> Vec<(String, String, ValidationError)> {
    let mut by_name: std::collections::HashMap<String, Vec<(String, String)>> =
        std::collections::HashMap::new();
    for group in groups {
        for table in &group.tables {
            if !table.deleted {
                by_name.entry(table.name.to_lowercase()).or_default()
                    .push((group.name.clone(), table.name.clone()));
            }
        }
        for constant in &group.constants {
            if !constant.deleted {
                by_name.entry(constant.name.to_lowercase()).or_default()
                    .push((group.name.clone(), constant.name.clone()));
            }
        }
        for enum_def in &group.enums {
            if !enum_def.deleted {
                by_name.entry(enum_def.name.to_lowercase()).or_default()
                    .push((group.name.clone(), enum_def.name.clone()));
            }
        }
    }

    let mut out = Vec::new();
    for occurrences in by_name.values() {
        if occurrences.len() < 2 {
            continue;
        }
        let labels = occurrences
            .iter()
            .map(|(g, n)| format!("{}/{}", g, n))
            .collect::<Vec<_>>()
            .join("、");
        for (group, node) in occurrences {
            let err = ValidationError::schema(
                ValidationCode::NodeNameDuplicate,
                format!(
                    "节点名 \"{}\" 在项目内重复（{}）；导出会平铺为同名文件/类",
                    node, labels
                ),
                0,
                None,
            )
            .with_field(node);
            out.push((group.clone(), node.clone(), err));
        }
    }
    out
}

/// 校验字段类型的引用目标（schema 层，不看具体值）。
/// 支持顶层 `@Xxx` 与复合类型内的嵌套 ref（`List<@Hero>` / `Map<int,@Hero>` ...）。
pub fn validate_ref_type(type_str: &str, refs: &RefIndex) -> Option<(ValidationCode, String)> {
    let t = TblType::parse(type_str)?;
    for name in t.ref_targets() {
        let bad = match refs.lookup(name) {
            None => Some((ValidationCode::TypeRefMissing,
                format!("引用的配置项 {} 不存在", name))),
            Some(RefKind::Constant) => Some((ValidationCode::TypeRefIsConstant,
                "不能引用 constant（无 id 概念）".to_string())),
            _ => None,
        };
        if let Some(b) = bad { return Some(b); }
    }
    None
}

/// 校验字段值里所有引用元素是否能在被引用项中找到。
/// 顶层 `@Xxx` 校验整体值；复合类型按结构切分逐元素校验。
fn validate_ref_value(tbl_type: &TblType, value: &str, sep: &SeparatorsSection, refs: &RefIndex) -> Option<(ValidationCode, String)> {
    if value.is_empty() { return None; }
    for (name, raw) in tbl_type.ref_elements(value, sep) {
        let bad = match refs.lookup(&name) {
            None => Some((ValidationCode::TypeRefMissing,
                format!("引用的配置项 {} 不存在", name))),
            Some(RefKind::Constant) => Some((ValidationCode::TypeRefIsConstant,
                "不能引用 constant".to_string())),
            Some(_) => {
                if refs.id_exists(&name, raw) { None }
                else { Some((ValidationCode::RefValueMissing,
                    format!("引用值 {} 不存在于 {}", raw, name))) }
            }
        };
        if let Some(b) = bad { return Some(b); }
    }
    None
}

pub fn col_letter(idx: usize) -> String {
    let mut result = String::new();
    let mut n = idx;
    loop {
        result.insert(0, (b'A' + (n % 26) as u8) as char);
        if n < 26 { break; }
        n = n / 26 - 1;
    }
    result
}

pub fn validate_table_cell(table: &Table, row: usize, col: usize, sep: &SeparatorsSection) -> Option<ValidationError> {
    validate_table_cell_with_refs(table, row, col, sep, None)
}

pub fn validate_table_cell_with_refs(
    table: &Table,
    row: usize,
    col: usize,
    sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
) -> Option<ValidationError> {
    let fields = &table.schema.fields;
    if col >= fields.len() { return None; }
    // 辅助列不参与校验（原始表头 / 类型 / export 都可能不合法）。
    if fields[col].is_aux() { return None; }
    let value = table.records.get(row).and_then(|r| r.get(col)).map(|s| s.as_str()).unwrap_or("");
    if value.is_empty() { return None; }
    let field_name = fields[col].name.as_str();

    let type_str = &fields[col].tbl_type;
    if let Some(tbl_type) = TblType::parse(type_str) {
        if let Some(msg) = tbl_type.validate_value(value, sep) {
            let code = if field_name == "id" { ValidationCode::IdNotInteger }
                else { ValidationCode::BaseTypeMismatch };
            return Some(ValidationError::cell(code, msg)
                .with_pos(row, col).with_field(field_name).with_value(value));
        }
        if let Some(refs) = refs {
            if let Some((code, msg)) = validate_ref_value(&tbl_type, value, sep, refs) {
                return Some(ValidationError::cell(code, msg)
                    .with_pos(row, col).with_field(field_name).with_value(value));
            }
        }
    }
    None
}

pub fn validate_table_row(table: &Table, row: usize, sep: &SeparatorsSection) -> Vec<ValidationError> {
    validate_table_row_with_refs(table, row, sep, None)
}

pub fn validate_table_row_with_refs(
    table: &Table,
    row: usize,
    sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let fields = &table.schema.fields;
    if table.records.get(row).is_none() { return errors; }

    for col in 0..fields.len() {
        if fields[col].is_aux() { continue; }
        if let Some(err) = validate_table_cell_with_refs(table, row, col, sep, refs) {
            errors.push(err);
        }
    }

    errors
}

pub fn validate_constant_cell(entry: &ConstEntry, col: usize, sep: &SeparatorsSection) -> Option<ValidationError> {
    validate_constant_cell_with_refs(entry, col, sep, None)
}

pub fn validate_constant_cell_with_refs(
    entry: &ConstEntry,
    col: usize,
    sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
) -> Option<ValidationError> {
    if col == ConstantCol::Name.col() {
        let name = &entry.name;
        if name.is_empty() { return None; }
        return validate_const_name(name).map(|(code, msg)| {
            ValidationError::cell(code, msg).with_col_field_value(col, name, name)
        });
    }
    if col == ConstantCol::Value.col() {
        let value = &entry.value;
        if value.is_empty() { return None; }
        if let Some(tbl_type) = TblType::parse(&entry.tbl_type) {
            if let Some(msg) = tbl_type.validate_value(value, sep) {
                return Some(ValidationError::cell(ValidationCode::BaseTypeMismatch, msg)
                    .with_col_field_value(col, &entry.name, value));
            }
            if let Some(refs) = refs {
                if let Some((code, msg)) = validate_ref_value(&tbl_type, value, sep, refs) {
                    return Some(ValidationError::cell(code, msg)
                        .with_col_field_value(col, &entry.name, value));
                }
            }
        }
    }
    None
}

pub fn validate_constant_row(constant: &Constant, row: usize, sep: &SeparatorsSection) -> Vec<ValidationError> {
    validate_constant_row_with_refs(constant, row, sep, None)
}

pub fn validate_constant_row_with_refs(
    constant: &Constant,
    row: usize,
    sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let entry = match constant.entries.get(row) { Some(e) => e, None => return errors };

    for col in 0..5 {
        if let Some(err) = validate_constant_cell_with_refs(entry, col, sep, refs) {
            errors.push(err.with_row(row));
        }
    }

    if !entry.name.is_empty() && entry.value.is_empty() {
        errors.push(
            ValidationError::cell(ValidationCode::NameFilledButValueEmpty, "name已填但value为空".to_string())
                .with_pos(row, ConstantCol::Value.col()).with_field(&entry.name)
        );
    }
    errors
}

pub fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() { return false; }
    let first = s.chars().next().unwrap();
    if !first.is_ascii_lowercase() { return false; }
    s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

pub fn is_java_keyword(s: &str) -> bool {
    matches!(s,
        "abstract" | "assert" | "boolean" | "break" | "byte" | "case" | "catch" |
        "char" | "class" | "const" | "continue" | "default" | "do" | "double" |
        "else" | "enum" | "extends" | "final" | "finally" | "float" | "for" |
        "goto" | "if" | "implements" | "import" | "instanceof" | "int" |
        "interface" | "long" | "native" | "new" | "package" | "private" |
        "protected" | "public" | "return" | "short" | "static" | "strictfp" |
        "super" | "switch" | "synchronized" | "this" | "throw" | "throws" |
        "transient" | "try" | "void" | "volatile" | "while" |
        "true" | "false" | "null"
    )
}

pub fn is_lua_keyword(s: &str) -> bool {
    matches!(s,
        "and" | "break" | "do" | "else" | "elseif" | "end" | "false" | "for" |
        "function" | "goto" | "if" | "in" | "local" | "nil" | "not" | "or" |
        "repeat" | "return" | "then" | "true" | "until" | "while"
    )
}

pub fn is_go_keyword(s: &str) -> bool {
    matches!(s,
        "break" | "case" | "chan" | "const" | "continue" | "default" | "defer" |
        "else" | "fallthrough" | "for" | "func" | "go" | "goto" | "if" |
        "import" | "interface" | "map" | "package" | "range" | "return" |
        "select" | "struct" | "switch" | "type" | "var"
    )
}

pub fn is_reserved_keyword(name: &str) -> bool {
    is_java_keyword(name) || is_lua_keyword(name) || is_go_keyword(name)
}

/// 返回命中的语言名（"Java/Go/Lua"），用于把"是关键字"的错误说清楚。
/// 没命中返回空字符串。
pub fn reserved_keyword_languages(name: &str) -> String {
    let mut langs = Vec::new();
    if is_java_keyword(name) { langs.push("Java"); }
    if is_go_keyword(name) { langs.push("Go"); }
    if is_lua_keyword(name) { langs.push("Lua"); }
    langs.join("/")
}

pub fn is_valid_group_name(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || ('\u{4e00}'..='\u{9fff}').contains(&c))
}

pub fn is_valid_node_name(s: &str) -> bool {
    if s.is_empty() { return false; }
    let first = s.chars().next().unwrap();
    if !first.is_ascii_uppercase() { return false; }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// 枚举条目命名规则：UPPER_SNAKE_CASE，[A-Z][A-Z0-9_]*
pub fn is_valid_enum_entry_name(s: &str) -> bool {
    if s.is_empty() { return false; }
    let first = s.chars().next().unwrap();
    if !first.is_ascii_uppercase() { return false; }
    s.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
}

pub fn validate_enum_cell(entry: &EnumEntry, col: usize) -> Option<ValidationError> {
    // 必填 / 唯一 / UPPER_SNAKE / id 正整数都已迁到内置规则（@01.8.11）；
    // 这里只保留「这个名字能不能生成代码」——它不是列值约束。
    if col == EnumCol::Name.col() && !entry.name.is_empty() && is_reserved_keyword(&entry.name) {
        return Some(
            ValidationError::cell(
                ValidationCode::FieldNameKeyword,
                format!("\"{}\" 是 {} 关键字", entry.name, reserved_keyword_languages(&entry.name)),
            ).with_col_field_value(col, &entry.name, &entry.name)
        );
    }
    None
}

pub fn validate_enum_row(enum_def: &EnumDef, row: usize) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let entry = match enum_def.entries.get(row) { Some(e) => e, None => return errors };

    for col in 0..3 {
        if let Some(err) = validate_enum_cell(entry, col) {
            errors.push(err.with_row(row));
        }
    }

    errors
}

// --- 字段名验证 ---

pub fn validate_field_name(name: &str) -> Option<(ValidationCode, String)> {
    if name.is_empty() {
        return Some((ValidationCode::FieldNameInvalid, "字段名不能为空".to_string()));
    }
    if !is_valid_identifier(name) {
        return Some((ValidationCode::FieldNameInvalid,
            format!("\"{}\" 不是合法字段名（需小写字母开头，只含小写字母/数字/下划线）", name)));
    }
    if is_reserved_keyword(name) {
        return Some((ValidationCode::FieldNameKeyword,
            format!("\"{}\" 是 {} 关键字", name, reserved_keyword_languages(name))));
    }
    None
}

pub fn validate_const_name(name: &str) -> Option<(ValidationCode, String)> {
    if name.is_empty() { return None; }
    if !is_valid_identifier(name) {
        return Some((ValidationCode::NameNotIdentifier,
            format!("\"{}\" 不是合法常量名（需小写字母开头，只含小写字母/数字/下划线）", name)));
    }
    if is_reserved_keyword(name) {
        return Some((ValidationCode::FieldNameKeyword,
            format!("\"{}\" 是 {} 关键字", name, reserved_keyword_languages(name))));
    }
    None
}

// --- 表头/Schema 验证 ---

pub fn validate_table_schema(table: &Table, sep: &SeparatorsSection) -> Vec<ValidationError> {
    validate_table_schema_with_refs(table, sep, None)
}

pub fn validate_table_schema_with_refs(
    table: &Table,
    _sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let fields = &table.schema.fields;

    if fields.is_empty() {
        errors.push(ValidationError::schema(
            ValidationCode::SchemaEmptyFields,
            "表没有定义任何字段".to_string(),
            0,
            Some(TableHeaderRow::Field),
        ));
        return errors;
    }

    let first_tbl = fields.iter().position(|f| !f.is_aux());
    if first_tbl.map_or(true, |i| fields[i].name != "id") {
        let pos = first_tbl.unwrap_or(0);
        errors.push(
            ValidationError::schema(
                ValidationCode::SchemaIdNotFirst,
                "第一列必须是主键 id".to_string(),
                pos,
                Some(TableHeaderRow::Field),
            ).with_field(fields.get(pos).map(|f| f.name.as_str()).unwrap_or(""))
        );
    }

    let mut seen_names = std::collections::HashSet::new();
    // 转换后键名（camelCase）→ 首次出现的源名，用于查「大小写转换后才撞」的情况
    let mut seen_keys: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (col, field) in fields.iter().enumerate() {
        if field.is_aux() { continue; }
        if let Some((code, msg)) = validate_field_name(&field.name) {
            errors.push(
                ValidationError::schema(code, msg, col, Some(TableHeaderRow::Field))
                    .with_field(&field.name)
            );
        }
        if !field.name.is_empty() && !seen_names.insert(field.name.clone()) {
            errors.push(
                ValidationError::schema(
                    ValidationCode::FieldNameDuplicate,
                    format!("字段名 \"{}\" 重复", field.name),
                    col,
                    Some(TableHeaderRow::Field),
                ).with_field(&field.name)
            );
        }
        // 源名各不相同但转换后同名（`stack_`→`stack`、`stack__max`→`stackMax`）：
        // 生成到各语言会出现重复成员，提前在验证层挡下（详见 @01.8.1）
        if !field.name.is_empty() {
            let key = crate::export::to_camel_case(&field.name);
            if let Some(prev) = seen_keys.insert(key.clone(), field.name.clone()) {
                errors.push(
                    ValidationError::schema(
                        ValidationCode::NameCollision,
                        crate::export::key_collision_message("字段名", &field.name, &prev, &key),
                        col,
                        Some(TableHeaderRow::Field),
                    ).with_field(&field.name)
                );
            }
        }
        if TblType::parse(&field.tbl_type).is_none() {
            errors.push(
                ValidationError::schema(
                    ValidationCode::TypeInvalid,
                    format!("类型 \"{}\" 不合法", field.tbl_type),
                    col,
                    Some(TableHeaderRow::Type),
                ).with_field(&field.name)
            );
        } else if let Some(refs) = refs {
            if let Some((code, msg)) = validate_ref_type(&field.tbl_type, refs) {
                errors.push(
                    ValidationError::schema(code, msg, col, Some(TableHeaderRow::Type))
                        .with_field(&field.name)
                );
            }
        }
    }

    errors
}

pub fn validate_constant_schema(constant: &Constant, _sep: &SeparatorsSection, allow_ref: bool) -> Vec<ValidationError> {
    let mut errors = Vec::new();

    let mut seen_names = std::collections::HashSet::new();
    let mut seen_keys: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for entry in &constant.entries {
        if entry.name.is_empty() { continue; }

        if let Some((code, msg)) = validate_const_name(&entry.name) {
            errors.push(
                ValidationError::schema(code, msg, ConstantCol::Name.col(), None)
                    .with_field(&entry.name)
            );
        }
        if !seen_names.insert(entry.name.clone()) {
            errors.push(
                ValidationError::schema(
                    ValidationCode::DuplicateName,
                    format!("常量名 \"{}\" 重复", entry.name),
                    ConstantCol::Name.col(),
                    None,
                ).with_field(&entry.name)
            );
        }
        // 同表字段：常量名也做大小写转换，转换后同名同样会生成出重复成员
        let key = crate::export::to_camel_case(&entry.name);
        if let Some(prev) = seen_keys.insert(key.clone(), entry.name.clone()) {
            errors.push(
                ValidationError::schema(
                    ValidationCode::NameCollision,
                    crate::export::key_collision_message("常量名", &entry.name, &prev, &key),
                    ConstantCol::Name.col(),
                    None,
                ).with_field(&entry.name)
            );
        }
        match TblType::parse(&entry.tbl_type) {
            None => errors.push(
                ValidationError::schema(
                    ValidationCode::TypeInvalid,
                    format!("类型 \"{}\" 不合法", entry.tbl_type),
                    ConstantCol::Type.col(),
                    None,
                ).with_field(&entry.name)
            ),
            Some(t) if t.paradigm == Paradigm::Ref && !allow_ref => errors.push(
                ValidationError::schema(
                    ValidationCode::ConstantRefForbidden,
                    "constant 不允许使用 @Xxx 引用类型".to_string(),
                    ConstantCol::Type.col(),
                    None,
                ).with_field(&entry.name)
            ),
            _ => {}
        }
    }

    errors
}

pub fn validate_enum_schema(enum_def: &EnumDef) -> Vec<ValidationError> {
    // 逐条的 id / name / 唯一性检查已迁到内置规则（@01.8.11）；
    // 这里只剩「一个枚举至少要有一个条目」这条结构性约束。
    if enum_def.entries.iter().all(|e| e.id.is_empty() && e.name.is_empty()) {
        return vec![ValidationError::schema(
            ValidationCode::EnumEmpty,
            "枚举至少需要一个条目".to_string(),
            EnumCol::Id.col(),
            None,
        )];
    }
    Vec::new()
}

// ────────────────────────────────────────────────────────────────────────────
// 整表层（节点级）：合并 schema + 行 + 跨行唯一性，输出 ValidationError 列表
// ────────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────────
// 节点级规则校验（@01.10.7）
// ────────────────────────────────────────────────────────────────────────────

/// 本地列名 → 0-based 列下标。
fn is_numeric(c: ValueClass) -> bool {
    matches!(c, ValueClass::Int | ValueClass::Float)
}

/// 这条验证器能否挂在这种类型的列上（UI 下拉过滤用）。
pub fn rule_kind_applies(kind: &RuleKind, class: ValueClass) -> bool {
    match kind {
        RuleKind::Range { .. } | RuleKind::Compare { .. } | RuleKind::PositiveInt { .. } =>
            is_numeric(class),
        RuleKind::Len { .. } | RuleKind::Matches { .. } | RuleKind::UpperSnake { .. } =>
            class == ValueClass::Text,
        _ => true,
    }
}

/// 给 GUI 用：草稿条件的 schema 错误。
pub fn condition_ui_error(condition: &Condition, columns: &[ColumnSpec]) -> Option<String> {
    crate::rule_engine::condition_ui_error(condition, columns)
}

/// 规则参数形状是否完整（UI 用；不检查列是否存在）。
pub fn rule_params_valid(expr: &RuleExpr) -> bool {
    crate::rule_engine::rule_params_valid(expr)
}

fn validate_node_rules<F, G>(
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
    crate::rule_engine::validate_node_rules(
        mode, node_name, columns, rules, cidx, row_count, cell, row_is_empty,
    )
}

pub fn validate_table(
    table: &Table,
    sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
    rules: Option<&RuleIndex>,
) -> Vec<ValidationError> {
    let mut errors = validate_table_schema_with_refs(table, sep, refs);
    for row in 0..table.records.len() {
        errors.extend(validate_table_row_with_refs(table, row, sep, refs));
    }
    {
        // 内置规则不需要项目上下文；单文件校验（rules=None）也要跑。
        let empty_idx = RuleIndex::build_schema(&[]);
        let cidx = rules.unwrap_or(&empty_idx);
        let columns = table_column_specs(table);
        // 有效规则 = 系统内置（id 的唯一/必填/正整数）+ 用户规则，
        // 内置校验已全部迁到规则引擎（@01.8.11）。
        let effective = effective_rules(NodeMode::Table, &table.rules);
        let mut rule_errors = validate_node_rules(
            NodeMode::Table, &table.name, &columns, &effective, cidx, table.records.len(),
            |r, c| {
                let idx = table.schema.fields.iter().position(|f| !f.is_aux() && f.name == c)?;
                table.records.get(r)?.get(idx).cloned()
            },
            |r| table.records.get(r).map_or(true, |row| {
                row.iter().all(|v| v.trim().is_empty())
            }),
        );
        // 类型检查已经报过的格子不再重复报规则错误（例如 "abc" 不是 int）。
        let occupied: std::collections::HashSet<(usize, usize)> = errors.iter()
            .filter(|e| !e.is_schema() && e.rule_index.is_none())
            .map(|e| (e.row, e.col))
            .collect();
        rule_errors.retain(|e| e.is_schema() || !occupied.contains(&(e.row, e.col)));
        errors.extend(rule_errors);
    }
    errors
}

/// Constant 的校验：内置检查（命名 / 类型 / 值 / 引用）+ 行级。
///
/// **常量不参与节点级规则**（@01.8.8）：`value` 列每行类型可以不同，
/// 列级规则没有统一的比较口径；内置校验已经覆盖它需要的约束。
pub fn validate_constant(
    constant: &Constant,
    sep: &SeparatorsSection,
    allow_ref: bool,
    refs: Option<&RefIndex>,
) -> Vec<ValidationError> {
    let mut errors = validate_constant_schema(constant, sep, allow_ref);
    for row in 0..constant.entries.len() {
        errors.extend(validate_constant_row_with_refs(constant, row, sep, refs));
    }
    errors
}

pub fn validate_enum(enum_def: &EnumDef, rules: Option<&RuleIndex>) -> Vec<ValidationError> {
    let mut errors = validate_enum_schema(enum_def);
    for row in 0..enum_def.entries.len() {
        errors.extend(validate_enum_row(enum_def, row));
    }
    {
        let empty_idx = RuleIndex::build_schema(&[]);
        let cidx = rules.unwrap_or(&empty_idx);
        let columns = enum_column_specs();
        // 有效规则 = 系统内置（id/name 的必填、唯一、正整数、UPPER_SNAKE）+ 用户规则。
        let effective = effective_rules(NodeMode::Enum, &enum_def.rules);
        let mut rule_errors = validate_node_rules(
            NodeMode::Enum, &enum_def.name, &columns, &effective, cidx, enum_def.entries.len(),
            |r, c| {
                let e = enum_def.entries.get(r)?;
                Some(match c {
                    "id" => e.id.clone(),
                    "name" => e.name.clone(),
                    "desc" => e.desc.clone(),
                    _ => return None,
                })
            },
            |r| enum_def.entries.get(r).map_or(true, |e| {
                e.id.trim().is_empty() && e.name.trim().is_empty() && e.desc.trim().is_empty()
            }),
        );
        // 关键字错误（非规则）优先：同一格不再重复报格式错误
        let occupied: std::collections::HashSet<(usize, usize)> = errors.iter()
            .filter(|e| !e.is_schema() && e.rule_index.is_none())
            .map(|e| (e.row, e.col))
            .collect();
        rule_errors.retain(|e| e.is_schema() || !occupied.contains(&(e.row, e.col)));
        errors.extend(rule_errors);
    }
    errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mk_table(name: &str, fields: &[(&str, &str)], records: &[Vec<&str>]) -> Table {
        Table {
            name: name.to_string(),
            original_name: name.to_string(),
            path: PathBuf::from(name),
            schema: TableSchema {
                fields: fields.iter().map(|(n, t)| FieldDef {
                    name: n.to_string(),
                    desc: String::new(),
                    tbl_type: t.to_string(),
                    export: Export::ClientServer,
                 aux: None, source_col: None,}).collect(),
            },
            records: records.iter().map(|r| r.iter().map(|s| s.to_string()).collect()).collect(),
            dirty: false,
            deleted: false,
            original_records: vec![],
            original_fields: vec![],
            saved: true,
            fmt_ver: crate::tbl::TblVersion::V3,
            display_field: None,
            original_display_field: None,

            rules: Vec::new(),
            original_rules: Vec::new(),
        }
    }

    fn mk_constant(name: &str, entries: &[&str]) -> Constant {
        Constant {
            name: name.to_string(),
            original_name: name.to_string(),
            path: PathBuf::from(name),
            entries: entries.iter().map(|n| ConstEntry {
                name: n.to_string(),
                tbl_type: "int".to_string(),
                value: "1".to_string(),
                export: Export::ClientServer,
                desc: String::new(),
            }).collect(),
            dirty: false,
            deleted: false,
            original_entries: vec![],
            saved: true,
        }
    }

    fn collision_hits(errs: &[ValidationError]) -> Vec<&ValidationError> {
        errs.iter().filter(|e| e.code == ValidationCode::NameCollision).collect()
    }

    #[test]
    fn transformed_name_collision_is_reported() {
        // `stack_` 与 `stack` 源名不同，但转换后都是 `stack` → 生成代码会出现重复字段
        let t = mk_table("T", &[("id", "int"), ("stack", "int"), ("stack_", "int")], &[]);
        let errs = validate_table_schema(&t, &SeparatorsSection::default());
        let hits = collision_hits(&errs);
        assert_eq!(hits.len(), 1, "应报 1 条撞名: {:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert_eq!(hits[0].field, "stack_");
        assert!(hits[0].message.contains("stack"), "got: {}", hits[0].message);
    }

    #[test]
    fn distinct_transformed_names_do_not_collide() {
        // `_` 后接字母、无结尾下划线也无连续下划线 → 转换后互不相同，不该误报
        let t = mk_table("T",
            &[("id", "int"), ("stage_1", "int"), ("stage1b", "int"), ("max_level", "int")], &[]);
        let errs = validate_table_schema(&t, &SeparatorsSection::default());
        assert!(collision_hits(&errs).is_empty(),
            "不该误报: {:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn constant_transformed_name_collision_is_reported() {
        let c = mk_constant("C", &["max_level", "max__level"]);
        let errs = validate_constant_schema(&c, &SeparatorsSection::default(), true);
        let hits = collision_hits(&errs);
        assert_eq!(hits.len(), 1, "应报 1 条撞名: {:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert_eq!(hits[0].field, "max__level");
    }

    #[test]
    fn nested_ref_value_existence_checked_per_element() {
        // Hero 表 id: 1001, 1002
        let hero = mk_table("Hero", &[("id", "int"), ("name", "str")], &[vec!["1001", "A"], vec!["1002", "B"]]);
        // Skill 表含 List<@Hero> 列
        let skill = mk_table("Skill", &[("id", "int"), ("heroes", "List<@Hero>")], &[
            vec!["1", "1001;1002"],   // 全部存在 → 合法
            vec!["2", "1001;9999"],   // 9999 不存在 → RefValueMissing
        ]);
        let groups = vec![Group {
            name: "g".into(), original_name: "g".into(), dir: PathBuf::from("g"),
            tables: vec![hero, skill], constants: vec![], enums: vec![], is_new: false,
            storage: crate::model::GroupStorage::Tbl, book_stamp: None,
        }];
        let refs = RefIndex::build(&groups);
        let sep = SeparatorsSection::default();

        let err = validate_table_cell_with_refs(&groups[0].tables[1], 0, 1, &sep, Some(&refs));
        assert!(err.is_none(), "合法 id 不应报错: {:?}", err);
        let err = validate_table_cell_with_refs(&groups[0].tables[1], 1, 1, &sep, Some(&refs));
        assert_eq!(err.map(|e| e.code), Some(ValidationCode::RefValueMissing));
    }

    #[test]
    fn nested_ref_schema_target_checked() {
        // 引用不存在的表 → TypeRefMissing
        let bad = validate_ref_type("List<@Missing>", &RefIndex::default());
        assert_eq!(bad.map(|(c, _)| c), Some(ValidationCode::TypeRefMissing));
        // 空 RefIndex 下，Map<int,@A> 的 @A 同样缺失
        let bad = validate_ref_type("Map<int,@A>", &RefIndex::default());
        assert_eq!(bad.map(|(c, _)| c), Some(ValidationCode::TypeRefMissing));
        // 纯 base 复合类型不触发 ref 校验
        assert_eq!(validate_ref_type("Map<int,int>", &RefIndex::default()), None);
    }

    // ── 节点级规则（@01.10.7） ──────────────────────────────────────────

    fn mk_group(name: &str, tables: Vec<Table>, enums: Vec<EnumDef>) -> Group {
        Group {
            name: name.to_string(),
            original_name: name.to_string(),
            dir: PathBuf::from(name),
            tables,
            constants: Vec::new(),
            enums,
            is_new: false,
            storage: crate::model::GroupStorage::Tbl,
            book_stamp: None,
        }
    }

    fn mk_enum(name: &str, entries: &[(&str, &str)]) -> EnumDef {
        EnumDef {
            name: name.to_string(),
            original_name: name.to_string(),
            path: PathBuf::from(name),
            entries: entries.iter().map(|(id, n)| EnumEntry {
                id: id.to_string(), name: n.to_string(), desc: String::new(),
            }).collect(),
            dirty: false,
            deleted: false,
            original_entries: vec![],
            saved: true,
            display_field: None,
            original_display_field: None,
            rules: Vec::new(),
            original_rules: Vec::new(),
        }
    }

    fn code_hits(errs: &[ValidationError], code: ValidationCode) -> usize {
        errs.iter().filter(|e| e.code == code).count()
    }
}
