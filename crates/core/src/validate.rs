use crate::model::*;
use crate::types::{BaseType, Paradigm, SeparatorsSection, TblType, TypeParam};

// ────────────────────────────────────────────────────────────────────────────
// 验证结构（cell / row / schema / table 各层共用）
// ────────────────────────────────────────────────────────────────────────────

/// 表头层错误使用的行号哨兵值，区分于普通数据行。
pub const SCHEMA_ROW: usize = usize::MAX;

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
    EnumIdInvalid,
    EnumNameInvalid,
    RefValueMissing,
    IdEmptyButHasData,
    NameFilledButValueEmpty,
    NameFilledButIdEmpty,
    DuplicateId,
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
}

impl ValidationError {
    pub fn is_schema(&self) -> bool { self.row == SCHEMA_ROW }

    /// cell 层用：仅有 code + message，行/列/字段由调用方在 row 层填充。
    fn cell(code: ValidationCode, message: String) -> Self {
        Self {
            code, severity: Severity::Error, row: 0, col: 0,
            header_row: None,
            field: String::new(),
            value: String::new(),
            message,
            rule_index: None,
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
    fn with_rule_index(mut self, idx: usize) -> Self {
        self.rule_index = Some(idx);
        self
    }

    fn with_pos(mut self, row: usize, col: usize) -> Self {
        self.row = row; self.col = col; self
    }

    fn with_row(mut self, row: usize) -> Self {
        self.row = row; self
    }

    fn with_field(mut self, field: impl Into<String>) -> Self {
        self.field = field.into(); self
    }

    fn with_value(mut self, value: impl Into<String>) -> Self {
        self.value = value.into(); self
    }

    /// Constant/Enum cell 层常用：一次填好 col + field + value（row 由 row 层补）。
    fn with_col_field_value(mut self, col: usize, field: impl Into<String>, value: impl Into<String>) -> Self {
        self.col = col;
        self.field = field.into();
        self.value = value.into();
        self
    }

    /// schema 层常用：标记为表头错误（row=SCHEMA_ROW），可选指定 Table 表头行号。
    fn schema(code: ValidationCode, message: String, col: usize, header_row: Option<TableHeaderRow>) -> Self {
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
                let idx = t.schema.fields.iter().position(|f| f.name == "id");
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
            Some(TypeParam::Base(b)) if matches!(b, BaseType::Int | BaseType::Long) =>
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
                let idx = t.schema.fields.iter().position(|f| f.name == column)?;
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
                if let RuleKind::RefExists { target_node, target_column } = &rule.kind {
                    wanted.insert((target_node.clone(), target_column.clone()));
                }
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
    let value = table.records.get(row).and_then(|r| r.get(col)).map(|s| s.as_str()).unwrap_or("");
    if value.is_empty() { return None; }
    let field_name = fields[col].name.as_str();

    if field_name == "id" {
        if value.parse::<i64>().is_err() {
            return Some(ValidationError::cell(ValidationCode::IdNotInteger, "ID必须是数字".to_string())
                .with_pos(row, col).with_field(field_name).with_value(value));
        }
    }

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
    let record = match table.records.get(row) { Some(r) => r, None => return errors };

    for col in 0..fields.len() {
        if let Some(err) = validate_table_cell_with_refs(table, row, col, sep, refs) {
            errors.push(err);
        }
    }

    let index_col = fields.iter().position(|f| f.name == "id");
    if let Some(idx_col) = index_col {
        let id_val = record.get(idx_col).map(|s| s.as_str()).unwrap_or("");
        if id_val.is_empty() {
            let has_data = record.iter().enumerate().any(|(i, v)| i != idx_col && !v.is_empty());
            if has_data {
                errors.push(ValidationError::cell(ValidationCode::IdEmptyButHasData, "有数据但ID为空".to_string())
                    .with_pos(row, idx_col).with_field("id"));
            }
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

pub fn validate_enum_entry_name(name: &str) -> Option<(ValidationCode, String)> {
    if name.is_empty() {
        return Some((ValidationCode::EnumNameInvalid, "枚举条目名不能为空".to_string()));
    }
    if !is_valid_enum_entry_name(name) {
        return Some((ValidationCode::EnumNameInvalid,
            format!("\"{}\" 不是合法枚举条目名（需大写字母开头，只含大写字母/数字/下划线）", name)));
    }
    if is_reserved_keyword(name) {
        return Some((ValidationCode::FieldNameKeyword,
            format!("\"{}\" 是 {} 关键字", name, reserved_keyword_languages(name))));
    }
    None
}

pub fn validate_enum_id(id: &str) -> Option<(ValidationCode, String)> {
    if id.is_empty() { return None; }
    match id.parse::<i32>() {
        Ok(v) if v < 0 => Some((ValidationCode::EnumIdInvalid, "id 必须是正整数".to_string())),
        Ok(0) => Some((ValidationCode::EnumIdInvalid, "id 不能为 0（保留为未设置语义）".to_string())),
        Ok(_) => None,
        Err(_) => Some((ValidationCode::EnumIdInvalid, "id 必须是正整数".to_string())),
    }
}

pub fn validate_enum_cell(entry: &EnumEntry, col: usize) -> Option<ValidationError> {
    if col == EnumCol::Id.col() {
        return validate_enum_id(&entry.id).map(|(code, msg)| {
            ValidationError::cell(code, msg).with_col_field_value(col, &entry.name, &entry.id)
        });
    }
    if col == EnumCol::Name.col() {
        if entry.name.is_empty() { return None; }
        return validate_enum_entry_name(&entry.name).map(|(code, msg)| {
            ValidationError::cell(code, msg).with_col_field_value(col, &entry.name, &entry.name)
        });
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

    if !entry.name.is_empty() && entry.id.is_empty() {
        errors.push(
            ValidationError::cell(ValidationCode::NameFilledButIdEmpty, "name已填但id为空".to_string())
                .with_pos(row, EnumCol::Id.col()).with_field(&entry.name)
        );
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

    if fields[0].name != "id" {
        errors.push(
            ValidationError::schema(
                ValidationCode::SchemaIdNotFirst,
                "第一列必须是主键 id".to_string(),
                0,
                Some(TableHeaderRow::Field),
            ).with_field(&fields[0].name)
        );
    }

    let mut seen_names = std::collections::HashSet::new();
    // 转换后键名（camelCase）→ 首次出现的源名，用于查「大小写转换后才撞」的情况
    let mut seen_keys: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for (col, field) in fields.iter().enumerate() {
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

    // 主键值重复检测：归到对应数据行（row != SCHEMA_ROW），而非表头错误
    if let Some(idx_col) = fields.iter().position(|f| f.name == "id") {
        let mut seen_ids: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for (row, record) in table.records.iter().enumerate() {
            let id = record.get(idx_col).map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() { continue; }
            if let Some(&first) = seen_ids.get(id) {
                errors.push(
                    ValidationError::cell(
                        ValidationCode::DuplicateId,
                        format!("ID \"{}\" 与第 {} 行重复", id, first + 1),
                    ).with_pos(row, idx_col).with_field("id").with_value(id)
                );
            } else {
                seen_ids.insert(id.to_string(), row);
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
    let mut errors = Vec::new();

    if enum_def.entries.iter().all(|e| e.id.is_empty() && e.name.is_empty()) {
        errors.push(ValidationError::schema(
            ValidationCode::EnumEmpty,
            "枚举至少需要一个条目".to_string(),
            EnumCol::Id.col(),
            None,
        ));
        return errors;
    }

    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_names = std::collections::HashSet::new();
    for entry in &enum_def.entries {
        if entry.id.is_empty() && entry.name.is_empty() { continue; }
        if let Some((code, msg)) = validate_enum_id(&entry.id) {
            errors.push(
                ValidationError::schema(code, msg, EnumCol::Id.col(), None)
                    .with_field(&entry.name)
            );
        }
        if let Some((code, msg)) = validate_enum_entry_name(&entry.name) {
            errors.push(
                ValidationError::schema(code, msg, EnumCol::Name.col(), None)
                    .with_field(&entry.name)
            );
        }
        if !entry.id.is_empty() && !seen_ids.insert(entry.id.clone()) {
            errors.push(
                ValidationError::schema(
                    ValidationCode::DuplicateId,
                    format!("id \"{}\" 重复", entry.id),
                    EnumCol::Id.col(),
                    None,
                ).with_field(&entry.name)
            );
        }
        if !entry.name.is_empty() && !seen_names.insert(entry.name.clone()) {
            errors.push(
                ValidationError::schema(
                    ValidationCode::DuplicateName,
                    format!("枚举条目名 \"{}\" 重复", entry.name),
                    EnumCol::Name.col(),
                    None,
                ).with_field(&entry.name)
            );
        }
    }

    errors
}

// ────────────────────────────────────────────────────────────────────────────
// 整表层（节点级）：合并 schema + 行 + 跨行唯一性，输出 ValidationError 列表
// ────────────────────────────────────────────────────────────────────────────

// ────────────────────────────────────────────────────────────────────────────
// 节点级规则校验（@01.10.7）
// ────────────────────────────────────────────────────────────────────────────

/// 本地列名 → 0-based 列下标。
fn rule_col_index(columns: &[ColumnSpec], column: &str) -> Option<usize> {
    columns.iter().position(|(c, _)| c == column)
}

fn is_numeric(c: ValueClass) -> bool {
    matches!(c, ValueClass::Int | ValueClass::Float)
}

/// 这条规则能不能挂在这种类型的列上。UI 下拉与校验层共用。
pub fn rule_kind_applies(kind: &RuleKind, class: ValueClass) -> bool {
    match kind {
        RuleKind::Range { .. } | RuleKind::Compare { .. } => is_numeric(class),
        RuleKind::Len { .. } | RuleKind::Matches { .. } => class == ValueClass::Text,
        _ => true,
    }
}

/// 解析区间端点：空串 = 不限。
fn parse_bound(raw: &str) -> Result<Option<f64>, String> {
    let t = raw.trim();
    if t.is_empty() { return Ok(None); }
    t.parse::<f64>()
        .map(Some)
        .map_err(|_| format!("\"{}\" 不是数字", t))
}

/// 区间参数校验：两端可空，填了必须是数字且 min <= max。
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

/// 规则能否真正执行：本地列存在且类型支持、参数合法、引用的列 / 目标可解析。
fn rule_is_usable(
    rule: &Rule,
    mode: NodeMode,
    node_name: &str,
    columns: &[ColumnSpec],
    cidx: &RuleIndex,
) -> bool {
    let Some(idx) = rule_col_index(columns, &rule.column) else { return false; };
    let Some(class) = columns[idx].1 else { return false; };
    if !rule_kind_applies(&rule.kind, class) { return false; }
    if rule.conflicts_with_builtin(mode) { return false; }
    match &rule.kind {
        RuleKind::RefExists { target_node, target_column } => {
            if !cidx.node_exists(target_node)
                || cidx.is_ambiguous(target_node)
                || !cidx.column_exists(target_node, target_column)
                || cidx.column_class(target_node, target_column).is_none() {
                return false;
            }
            if target_node == node_name && target_column == &rule.column { return false; }
        }
        RuleKind::Range { min, max } | RuleKind::Len { min, max } => {
            if check_bounds(min, max).is_err() { return false; }
        }
        RuleKind::OneOf { values } => {
            if values.is_empty() { return false; }
        }
        RuleKind::Matches { pattern } => {
            if regex::Regex::new(pattern).is_err() { return false; }
        }
        RuleKind::RequiredIf { column, .. } => {
            if column == &rule.column || rule_col_index(columns, column).is_none() { return false; }
        }
        RuleKind::Compare { column, .. } => {
            if column == &rule.column { return false; }
            let Some(other) = rule_col_index(columns, column) else { return false; };
            let Some(other_class) = columns[other].1 else { return false; };
            if !is_numeric(other_class) { return false; }
        }
        _ => {}
    }
    true
}

/// 规则的 schema 层校验：只回答「这条规则本身能不能跑」，不看数据行。
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
        let col_idx = match rule_col_index(columns, &rule.column) {
            Some(i) => i,
            None => {
                errors.push(
                    ValidationError::schema(
                        ValidationCode::RuleColumnMissing,
                        format!("规则引用的列 \"{}\" 不存在", rule.column),
                        0, None,
                    ).with_field(rule.column.as_str()).with_level(level)
                     .with_rule_index(rule_idx)
                );
                continue;
            }
        };
        let Some(class) = columns[col_idx].1 else {
            errors.push(
                ValidationError::schema(
                    ValidationCode::RuleTypeIncompatible,
                    format!(
                        "列 \"{}\" 的类型不支持校验规则（List / Set / Map / Tuple 等复合类型不参与）",
                        rule.column
                    ),
                    col_idx, None,
                ).with_field(rule.column.as_str()).with_level(level)
                 .with_rule_index(rule_idx)
            );
            continue;
        };
        if !rule_kind_applies(&rule.kind, class) {
            errors.push(
                ValidationError::schema(
                    ValidationCode::RuleTypeIncompatible,
                    format!(
                        "{} 不适用于列 \"{}\" 的类型（{}）",
                        rule.expr_string(), rule.column, class_name(class)
                    ),
                    col_idx, None,
                ).with_field(rule.column.as_str()).with_level(level)
                 .with_rule_index(rule_idx)
            );
            continue;
        }
        if rule.conflicts_with_builtin(mode) {
            errors.push(
                ValidationError::schema(
                    ValidationCode::RuleRedundant,
                    format!(
                        "列 \"{}\" 已由系统保证唯一，{} 不会生效",
                        rule.column, rule.expr_string()
                    ),
                    col_idx, None,
                ).with_field(rule.column.as_str()).with_level(Severity::Warning)
                 .with_rule_index(rule_idx)
            );
            continue;
        }

        let mut bad: Option<(ValidationCode, String)> = None;
        match &rule.kind {
            RuleKind::RefExists { target_node, target_column } => {
                let target = format!("{}.{}", target_node, target_column);
                bad = if target_node == node_name && target_column == &rule.column {
                    Some((ValidationCode::RuleParamInvalid,
                        "ref_exists 的目标不能是本列自己（恒成立，没有意义）".to_string()))
                } else if !cidx.node_exists(target_node) {
                    Some((ValidationCode::RuleTargetMissing,
                        format!("ref_exists 的目标节点 \"{}\" 不存在", target_node)))
                } else if cidx.is_ambiguous(target_node) {
                    Some((ValidationCode::RuleTargetAmbiguous,
                        format!("ref_exists 的目标节点 \"{}\" 在项目里重名", target_node)))
                } else if !cidx.column_exists(target_node, target_column) {
                    Some((ValidationCode::RuleTargetColumnMissing,
                        format!("ref_exists 的目标列 \"{}\" 不存在", target)))
                } else if cidx.column_class(target_node, target_column).is_none() {
                    Some((ValidationCode::RuleTypeIncompatible,
                        format!("ref_exists 的目标列 \"{}\" 是复合类型，不支持引用校验", target)))
                } else {
                    None
                };
            }
            RuleKind::Range { min, max } | RuleKind::Len { min, max } => {
                if let Err(msg) = check_bounds(min, max) {
                    bad = Some((ValidationCode::RuleParamInvalid, msg));
                }
            }
            RuleKind::OneOf { values } => {
                if values.is_empty() {
                    bad = Some((ValidationCode::RuleParamInvalid, "one_of 至少需要一个候选值".to_string()));
                }
            }
            RuleKind::Matches { pattern } => {
                if pattern.trim().is_empty() {
                    bad = Some((ValidationCode::RuleParamInvalid,
                        "正则不能为空（空正则匹配一切，等于没写）".to_string()));
                } else if let Err(e) = regex::Regex::new(pattern) {
                    bad = Some((ValidationCode::RuleParamInvalid, format!("正则不合法: {}", e)));
                }
            }
            RuleKind::RequiredIf { column, .. } => {
                if column == &rule.column {
                    bad = Some((ValidationCode::RuleParamInvalid,
                        "required_if 引用的列不能是本列自己".to_string()));
                } else if rule_col_index(columns, column).is_none() {
                    bad = Some((ValidationCode::RuleColumnMissing,
                        format!("required_if 引用的列 \"{}\" 不存在", column)));
                }
            }
            RuleKind::Compare { column, .. } => {
                if column == &rule.column {
                    bad = Some((ValidationCode::RuleParamInvalid,
                        "compare 引用的列不能是本列自己".to_string()));
                } else if let Some(other) = rule_col_index(columns, column) {
                    match columns[other].1 {
                        Some(c) if is_numeric(c) => {}
                        _ => bad = Some((ValidationCode::RuleTypeIncompatible,
                            format!("compare 引用的列 \"{}\" 不是数值类型", column))),
                    }
                } else {
                    bad = Some((ValidationCode::RuleColumnMissing,
                        format!("compare 引用的列 \"{}\" 不存在", column)));
                }
            }
            _ => {}
        }
        if let Some((code, msg)) = bad {
            errors.push(
                ValidationError::schema(code, msg, col_idx, None)
                    .with_field(rule.column.as_str())
                    .with_level(level)
                    .with_rule_index(rule_idx)
            );
        }
    }
    errors
}

/// 规则参数本身是否合法（UI 用来决定「添加 / 保存」是否可用）。
pub fn rule_params_valid(kind: &RuleKind) -> bool {
    match kind {
        RuleKind::Range { min, max } | RuleKind::Len { min, max } => check_bounds(min, max).is_ok(),
        RuleKind::OneOf { values } => !values.is_empty(),
        RuleKind::Matches { pattern } =>
            !pattern.trim().is_empty() && regex::Regex::new(pattern).is_ok(),
        RuleKind::RefExists { target_node, target_column } =>
            !target_node.is_empty() && !target_column.is_empty(),
        RuleKind::RequiredIf { column, .. } | RuleKind::Compare { column, .. } =>
            !column.is_empty(),
        _ => true,
    }
}

fn class_name(class: ValueClass) -> &'static str {
    match class {
        ValueClass::Int => "整数",
        ValueClass::Float => "浮点",
        ValueClass::Text => "文本",
    }
}

/// 规则的数据行校验。只跑 `rule_is_usable` 通过的规则。
fn validate_rule_rows(
    mode: NodeMode,
    node_name: &str,
    columns: &[ColumnSpec],
    rules: &[Rule],
    cidx: &RuleIndex,
    row_count: usize,
    cell: impl Fn(usize, &str) -> Option<String>,
    row_is_empty: impl Fn(usize) -> bool,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    for (rule_idx, rule) in rules.iter().enumerate() {
        if !rule_is_usable(rule, mode, node_name, columns, cidx) { continue; }
        let col_idx = match rule_col_index(columns, &rule.column) { Some(i) => i, None => continue };
        let level = rule.level;
        let src_class = columns[col_idx].1.unwrap_or(ValueClass::Text);
        let mut push = |row: usize, code: ValidationCode, msg: String, value: &str| {
            errors.push(
                ValidationError::cell(code, msg)
                    .with_pos(row, col_idx)
                    .with_field(rule.column.as_str())
                    .with_value(value)
                    .with_level(level)
                    .with_rule_index(rule_idx)
            );
        };
        match &rule.kind {
            RuleKind::Unique => {
                let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
                for row in 0..row_count {
                    let v = match cell(row, &rule.column) { Some(v) => v, None => continue };
                    let nv = src_class.normalize(&v);
                    if nv.is_empty() { continue; }
                    if let Some(&first) = seen.get(&nv) {
                        push(row, ValidationCode::RuleUniqueViolation,
                            format!("值 \"{}\" 与第 {} 行重复", nv, first + 1), &nv);
                    } else {
                        seen.insert(nv, row);
                    }
                }
            }
            RuleKind::RefExists { target_node, target_column } => {
                let (tclass, set) = match cidx.values(target_node, target_column) {
                    Some(x) => x,
                    None => continue,
                };
                for row in 0..row_count {
                    let v = match cell(row, &rule.column) { Some(v) => v, None => continue };
                    let nv = tclass.normalize(&v);
                    if nv.is_empty() { continue; }
                    if !set.contains(&nv) {
                        push(row, ValidationCode::RuleRefValueMissing,
                            format!("引用值 \"{}\" 不存在于 {}.{}", nv, target_node, target_column), &nv);
                    }
                }
            }
            RuleKind::Range { min, max } => {
                let lo = parse_bound(min).ok().flatten();
                let hi = parse_bound(max).ok().flatten();
                for row in 0..row_count {
                    let v = match cell(row, &rule.column) { Some(v) => v, None => continue };
                    let t = v.trim();
                    if t.is_empty() { continue; }
                    let Some(n) = t.parse::<f64>().ok() else {
                        push(row, ValidationCode::RuleRangeViolation,
                            format!("值 \"{}\" 不是数字，无法做区间校验", t), t);
                        continue;
                    };
                    let below = lo.map_or(false, |b| n < b);
                    let above = hi.map_or(false, |b| n > b);
                    if below || above {
                        let range = match (lo, hi) {
                            (Some(a), Some(b)) => format!("[{}, {}]", a, b),
                            (Some(a), None) => format!("≥ {}", a),
                            (None, Some(b)) => format!("≤ {}", b),
                            (None, None) => String::new(),
                        };
                        push(row, ValidationCode::RuleRangeViolation,
                            format!("值 {} 不在允许区间 {} 内", n, range), t);
                    }
                }
            }
            RuleKind::NonEmpty => {
                for row in 0..row_count {
                    // 整行全空 = 占位行（表尾/枚举尾的空行），不算缺填。
                    if row_is_empty(row) { continue; }
                    let v = match cell(row, &rule.column) { Some(v) => v, None => continue };
                    if v.trim().is_empty() {
                        push(row, ValidationCode::RuleNotEmptyViolation, "该列必填，不能为空".to_string(), "");
                    }
                }
            }
            RuleKind::Len { min, max } => {
                let lo = parse_bound(min).ok().flatten();
                let hi = parse_bound(max).ok().flatten();
                for row in 0..row_count {
                    let v = match cell(row, &rule.column) { Some(v) => v, None => continue };
                    if v.is_empty() { continue; }
                    let n = v.chars().count() as f64;
                    let below = lo.map_or(false, |b| n < b);
                    let above = hi.map_or(false, |b| n > b);
                    if below || above {
                        let range = match (lo, hi) {
                            (Some(a), Some(b)) => format!("[{}, {}]", a, b),
                            (Some(a), None) => format!("≥ {}", a),
                            (None, Some(b)) => format!("≤ {}", b),
                            (None, None) => String::new(),
                        };
                        push(row, ValidationCode::RuleLenViolation,
                            format!("长度 {} 不在允许区间 {} 内", n as i64, range), &v);
                    }
                }
            }
            RuleKind::OneOf { values } => {
                let set: std::collections::HashSet<String> =
                    values.iter().map(|v| src_class.normalize(v)).collect();
                for row in 0..row_count {
                    let v = match cell(row, &rule.column) { Some(v) => v, None => continue };
                    let nv = src_class.normalize(&v);
                    if nv.is_empty() { continue; }
                    if !set.contains(&nv) {
                        push(row, ValidationCode::RuleOneOfViolation,
                            format!("值 \"{}\" 不在允许集合 {{{}}} 内", nv, values.join(", ")), &nv);
                    }
                }
            }
            RuleKind::Matches { pattern } => {
                let Ok(re) = regex::Regex::new(pattern) else { continue };
                for row in 0..row_count {
                    let v = match cell(row, &rule.column) { Some(v) => v, None => continue };
                    if v.is_empty() { continue; }
                    if !re.is_match(&v) {
                        push(row, ValidationCode::RulePatternViolation,
                            format!("值 \"{}\" 不匹配正则 {}", v, pattern), &v);
                    }
                }
            }
            RuleKind::RequiredIf { column, value } => {
                let Some(other_idx) = rule_col_index(columns, column) else { continue };
                let other_class = columns[other_idx].1.unwrap_or(ValueClass::Text);
                let want = other_class.normalize(value);
                for row in 0..row_count {
                    if row_is_empty(row) { continue; }
                    let other = match cell(row, column) { Some(v) => v, None => continue };
                    if other_class.normalize(&other) != want { continue; }
                    let v = cell(row, &rule.column).unwrap_or_default();
                    if v.trim().is_empty() {
                        push(row, ValidationCode::RuleRequiredIfMissing,
                            format!("当 {} = {} 时，本列必填", column, value), "");
                    }
                }
            }
            RuleKind::Compare { column, op } => {
                for row in 0..row_count {
                    let left = match cell(row, &rule.column) { Some(v) => v, None => continue };
                    let right = match cell(row, column) { Some(v) => v, None => continue };
                    let lt = left.trim();
                    let rt = right.trim();
                    if lt.is_empty() || rt.is_empty() { continue; }
                    let (Some(a), Some(b)) = (lt.parse::<f64>().ok(), rt.parse::<f64>().ok()) else {
                        push(row, ValidationCode::RuleCompareViolation,
                            format!("值 \"{}\" 或 \"{}\" 不是数字，无法比较", lt, rt), lt);
                        continue;
                    };
                    let ok = match op {
                        CompareOp::Lt => a < b,
                        CompareOp::Le => a <= b,
                        CompareOp::Gt => a > b,
                        CompareOp::Ge => a >= b,
                        CompareOp::Eq => a == b,
                        CompareOp::Ne => a != b,
                    };
                    if !ok {
                        push(row, ValidationCode::RuleCompareViolation,
                            format!("本列值 {} 应 {} {} 列的值 {}", a, op.as_str(), column, b), lt);
                    }
                }
            }
        }
    }
    errors
}

/// 三种模式共用的规则校验入口（schema + 行）。
fn validate_node_rules(
    mode: NodeMode,
    node_name: &str,
    columns: &[ColumnSpec],
    rules: &[Rule],
    cidx: &RuleIndex,
    row_count: usize,
    cell: impl Fn(usize, &str) -> Option<String>,
    row_is_empty: impl Fn(usize) -> bool,
) -> Vec<ValidationError> {
    let mut errors = validate_rules_schema(mode, node_name, columns, rules, cidx);
    errors.extend(validate_rule_rows(
        mode, node_name, columns, rules, cidx, row_count, cell, row_is_empty,
    ));
    errors
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
    if let Some(cidx) = rules {
        let columns = table_column_specs(table);
        errors.extend(validate_node_rules(
            NodeMode::Table, &table.name, &columns, &table.rules, cidx, table.records.len(),
            |r, c| {
                let idx = table.schema.fields.iter().position(|f| f.name == c)?;
                table.records.get(r)?.get(idx).cloned()
            },
            |r| table.records.get(r).map_or(true, |row| {
                row.iter().all(|v| v.trim().is_empty())
            }),
        ));
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
    if let Some(cidx) = rules {
        let columns = enum_column_specs();
        errors.extend(validate_node_rules(
            NodeMode::Enum, &enum_def.name, &columns, &enum_def.rules, cidx, enum_def.entries.len(),
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
        ));
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
                }).collect(),
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

    #[test]
    fn unique_rule_reports_duplicate_and_skips_empty() {
        let mut t = mk_table("T", &[("id", "int"), ("code", "str")],
            &[vec!["1", "A"], vec!["2", "A"], vec!["3", ""], vec!["4", ""]]);
        t.rules = vec![Rule { column: "code".into(), kind: RuleKind::Unique, level: Severity::Error }];
        let groups = vec![mk_group("g", vec![t], vec![])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_table(&groups[0].tables[0], &SeparatorsSection::default(), None, Some(&idx));
        let hits: Vec<_> = errs.iter()
            .filter(|e| e.code == ValidationCode::RuleUniqueViolation)
            .collect();
        assert_eq!(hits.len(), 1, "只有第 2 行重复: {:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert_eq!(hits[0].row, 1);
        assert_eq!(hits[0].rule_index, Some(0));
        assert_eq!(hits[0].severity, Severity::Error);
    }

    #[test]
    fn unique_rule_on_system_unique_column_is_redundant_warning() {
        let mut e = mk_enum("E", &[("1", "A"), ("2", "B")]);
        e.rules = vec![Rule { column: "name".into(), kind: RuleKind::Unique, level: Severity::Error }];
        let groups = vec![mk_group("g", vec![], vec![e])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_enum(&groups[0].enums[0], Some(&idx));
        assert_eq!(code_hits(&errs, ValidationCode::RuleRedundant), 1);
        // 冗余规则不生效，所以不会有真正的重复错误
        assert_eq!(code_hits(&errs, ValidationCode::RuleUniqueViolation), 0);
        let hit = errs.iter().find(|e| e.code == ValidationCode::RuleRedundant).unwrap();
        assert_eq!(hit.severity, Severity::Warning);
    }

    #[test]
    fn ref_exists_reports_missing_value() {
        let mut lang = mk_table("Lang", &[("id", "int"), ("key", "str")],
            &[vec!["1", "PLAYER_NOT_FOUND"], vec!["2", "PARAM_INVALID"]]);
        lang.rules = vec![Rule { column: "key".into(), kind: RuleKind::Unique, level: Severity::Error }];
        let mut code = mk_enum("ErrorCode", &[("10001", "PLAYER_NOT_FOUND"), ("10002", "MISSING")]);
        code.rules = vec![Rule {
            column: "name".into(),
            kind: RuleKind::RefExists { target_node: "Lang".into(), target_column: "key".into() },
            level: Severity::Warning,
        }];
        let groups = vec![mk_group("g", vec![lang], vec![code])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_enum(&groups[0].enums[0], Some(&idx));
        let hits: Vec<_> = errs.iter()
            .filter(|e| e.code == ValidationCode::RuleRefValueMissing)
            .collect();
        assert_eq!(hits.len(), 1, "{:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert_eq!(hits[0].row, 1);
        assert_eq!(hits[0].severity, Severity::Warning);
    }

    #[test]
    fn ref_exists_missing_target_uses_rule_level() {
        let mut code = mk_enum("ErrorCode", &[("10001", "PLAYER_NOT_FOUND")]);
        code.rules = vec![Rule {
            column: "name".into(),
            kind: RuleKind::RefExists { target_node: "Lang".into(), target_column: "key".into() },
            level: Severity::Warning,
        }];
        let groups = vec![mk_group("g", vec![], vec![code])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_enum(&groups[0].enums[0], Some(&idx));
        let hit = errs.iter().find(|e| e.code == ValidationCode::RuleTargetMissing).unwrap();
        assert_eq!(hit.severity, Severity::Warning);
        assert!(hit.is_schema());
    }

    #[test]
    fn rule_missing_local_column_is_reported() {
        let mut t = mk_table("T", &[("id", "int")], &[]);
        t.rules = vec![Rule { column: "nope".into(), kind: RuleKind::Unique, level: Severity::Error }];
        let groups = vec![mk_group("g", vec![t], vec![])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_table(&groups[0].tables[0], &SeparatorsSection::default(), None, Some(&idx));
        assert_eq!(code_hits(&errs, ValidationCode::RuleColumnMissing), 1);
    }

    #[test]
    fn non_empty_skips_fully_empty_placeholder_rows() {
        let mut t = mk_table("T", &[("id", "int"), ("name", "str")],
            &[vec!["1", "A"], vec!["2", ""], vec!["", ""]]);
        t.rules = vec![Rule { column: "name".into(), kind: RuleKind::NonEmpty, level: Severity::Error }];
        let groups = vec![mk_group("g", vec![t], vec![])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_table(&groups[0].tables[0], &SeparatorsSection::default(), None, Some(&idx));
        let hits: Vec<_> = errs.iter().filter(|e| e.code == ValidationCode::RuleNotEmptyViolation).collect();
        assert_eq!(hits.len(), 1, "只有第 2 行缺填: {:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert_eq!(hits[0].row, 1);
    }

    #[test]
    fn range_len_one_of_matches_report_violations() {
        let mut t = mk_table("T",
            &[("id", "int"), ("hp", "int"), ("code", "str"), ("kind", "str")],
            &[vec!["1", "120", "AB", "A"], vec!["2", "50", "x", "Z"]]);
        t.rules = vec![
            Rule { column: "hp".into(), kind: RuleKind::Range { min: "0".into(), max: "100".into() }, level: Severity::Error },
            Rule { column: "code".into(), kind: RuleKind::Len { min: "2".into(), max: "4".into() }, level: Severity::Error },
            Rule { column: "kind".into(), kind: RuleKind::OneOf { values: vec!["A".into(), "B".into()] }, level: Severity::Error },
            Rule { column: "code".into(), kind: RuleKind::Matches { pattern: "^[A-Z]+$".into() }, level: Severity::Error },
        ];
        let groups = vec![mk_group("g", vec![t], vec![])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_table(&groups[0].tables[0], &SeparatorsSection::default(), None, Some(&idx));
        assert_eq!(code_hits(&errs, ValidationCode::RuleRangeViolation), 1, "{:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert_eq!(code_hits(&errs, ValidationCode::RuleLenViolation), 1);
        assert_eq!(code_hits(&errs, ValidationCode::RuleOneOfViolation), 1);
        assert_eq!(code_hits(&errs, ValidationCode::RulePatternViolation), 1);
    }

    #[test]
    fn required_if_and_compare_report_violations() {
        let mut t = mk_table("T",
            &[("id", "int"), ("type", "int"), ("skill", "int"), ("min_lv", "int"), ("max_lv", "int")],
            &[vec!["1", "2", "10", "1", "5"], vec!["2", "2", "", "9", "5"], vec!["3", "0", "", "1", "5"]]);
        t.rules = vec![
            Rule { column: "skill".into(),
                   kind: RuleKind::RequiredIf { column: "type".into(), value: "2".into() },
                   level: Severity::Error },
            Rule { column: "min_lv".into(),
                   kind: RuleKind::Compare { column: "max_lv".into(), op: CompareOp::Le },
                   level: Severity::Error },
        ];
        let groups = vec![mk_group("g", vec![t], vec![])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_table(&groups[0].tables[0], &SeparatorsSection::default(), None, Some(&idx));
        assert_eq!(code_hits(&errs, ValidationCode::RuleRequiredIfMissing), 1, "{:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
        assert_eq!(code_hits(&errs, ValidationCode::RuleCompareViolation), 1);
    }

    #[test]
    fn self_references_and_empty_params_are_rejected() {
        let mut t = mk_table("T", &[("id", "int"), ("parent", "int"), ("code", "str")],
            &[vec!["1", "1", "A"]]);
        t.rules = vec![
            // ref_exists 自己指自己
            Rule { column: "id".into(),
                   kind: RuleKind::RefExists { target_node: "T".into(), target_column: "id".into() },
                   level: Severity::Error },
            // compare 自己比自己
            Rule { column: "parent".into(),
                   kind: RuleKind::Compare { column: "parent".into(), op: CompareOp::Le },
                   level: Severity::Error },
            // required_if 引用本列
            Rule { column: "code".into(),
                   kind: RuleKind::RequiredIf { column: "code".into(), value: "A".into() },
                   level: Severity::Error },
            // 两端都空 / 空正则
            Rule { column: "parent".into(), kind: RuleKind::Range { min: "".into(), max: "".into() }, level: Severity::Error },
            Rule { column: "code".into(), kind: RuleKind::Matches { pattern: "".into() }, level: Severity::Error },
        ];
        let groups = vec![mk_group("g", vec![t], vec![])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_table(&groups[0].tables[0], &SeparatorsSection::default(), None, Some(&idx));
        assert_eq!(code_hits(&errs, ValidationCode::RuleParamInvalid), 5,
            "{:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn value_class_of_type_classifies_scalars_and_rejects_composites() {
        assert_eq!(value_class_of_type("int"), Some(ValueClass::Int));
        assert_eq!(value_class_of_type("long"), Some(ValueClass::Int));
        assert_eq!(value_class_of_type("str"), Some(ValueClass::Text));
        assert_eq!(value_class_of_type("txt"), Some(ValueClass::Text));
        assert_eq!(value_class_of_type("float"), Some(ValueClass::Float));
        assert_eq!(value_class_of_type("@HeroType"), Some(ValueClass::Text));
        assert_eq!(value_class_of_type("List<int>"), None);
        assert_eq!(value_class_of_type("Map<int,str>"), None);
        assert_eq!(value_class_of_type("Tuple2<int,int>"), None);
    }

    #[test]
    fn unique_compares_integers_numerically() {
        let mut t = mk_table("T", &[("id", "int"), ("code", "int")],
            &[vec!["1", "01"], vec!["2", "1"], vec!["3", "2"]]);
        t.rules = vec![Rule { column: "code".into(), kind: RuleKind::Unique, level: Severity::Error }];
        let groups = vec![mk_group("g", vec![t], vec![])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_table(&groups[0].tables[0], &SeparatorsSection::default(), None, Some(&idx));
        assert_eq!(code_hits(&errs, ValidationCode::RuleUniqueViolation), 1,
            "01 与 1 应视为重复: {:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn rule_on_composite_column_is_incompatible() {
        let mut t = mk_table("T", &[("id", "int"), ("tags", "List<int>")], &[]);
        t.rules = vec![Rule { column: "tags".into(), kind: RuleKind::Unique, level: Severity::Error }];
        let groups = vec![mk_group("g", vec![t], vec![])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_table(&groups[0].tables[0], &SeparatorsSection::default(), None, Some(&idx));
        assert_eq!(code_hits(&errs, ValidationCode::RuleTypeIncompatible), 1);
        assert_eq!(code_hits(&errs, ValidationCode::RuleUniqueViolation), 0);
    }

    #[test]
    fn ref_exists_compares_integers_numerically() {
        let lang = mk_table("Lang", &[("id", "int")], &[vec!["01"], vec!["2"]]);
        let mut code = mk_table("Code", &[("id", "int"), ("target", "int")],
            &[vec!["1", "1"], vec!["2", "2"]]);
        code.rules = vec![Rule {
            column: "target".into(),
            kind: RuleKind::RefExists { target_node: "Lang".into(), target_column: "id".into() },
            level: Severity::Error,
        }];
        let groups = vec![mk_group("g", vec![lang, code], vec![])];
        let idx = RuleIndex::build(&groups);
        let errs = validate_table(&groups[0].tables[1], &SeparatorsSection::default(), None, Some(&idx));
        assert_eq!(code_hits(&errs, ValidationCode::RuleRefValueMissing), 0,
            "1 应匹配 01: {:?}", errs.iter().map(|e| &e.message).collect::<Vec<_>>());
    }

    #[test]
    fn rule_index_only_scans_referenced_target_columns() {
        let lang = mk_table("Lang", &[("id", "int"), ("key", "str")],
            &[vec!["1", "A"]]);
        let mut code = mk_enum("ErrorCode", &[("1", "A")]);
        code.rules = vec![Rule {
            column: "name".into(),
            kind: RuleKind::RefExists { target_node: "Lang".into(), target_column: "key".into() },
            level: Severity::Error,
        }];
        let groups = vec![mk_group("g", vec![lang], vec![code])];
        let idx = RuleIndex::build(&groups);
        assert!(idx.values("Lang", "key").is_some());
        // 没被任何规则引用的列不建值集
        assert!(idx.values("Lang", "id").is_none());
        // schema 层仍然知道列存在
        assert!(idx.column_exists("Lang", "id"));
    }
}
