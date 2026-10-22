use crate::model::*;
use crate::types::{TblType, Paradigm, SeparatorsSection};

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
    Other,
}

/// 一条验证错误（atomic）：含错误码、定位信息、原值与人类可读消息。
/// 表示"无错误"用 `Option::None` 或空 `Vec`，不在本结构内体现。
#[derive(Clone, Debug)]
pub struct ValidationError {
    pub code: ValidationCode,
    /// SCHEMA_ROW 表示表头层错误，其它为 0-based 数据行索引。
    pub row: usize,
    pub col: usize,
    /// 仅 Table schema 层有意义；Constant/Enum schema 与数据行均为 None。
    pub header_row: Option<TableHeaderRow>,
    pub field: String,
    pub value: String,
    pub message: String,
}

impl ValidationError {
    pub fn is_schema(&self) -> bool { self.row == SCHEMA_ROW }

    /// cell 层用：仅有 code + message，行/列/字段由调用方在 row 层填充。
    fn cell(code: ValidationCode, message: String) -> Self {
        Self {
            code, row: 0, col: 0,
            header_row: None,
            field: String::new(),
            value: String::new(),
            message,
        }
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
            row: SCHEMA_ROW,
            col,
            header_row,
            field: String::new(),
            value: String::new(),
            message,
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

pub fn validate_table(
    table: &Table,
    sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
) -> Vec<ValidationError> {
    let mut errors = validate_table_schema_with_refs(table, sep, refs);
    for row in 0..table.records.len() {
        errors.extend(validate_table_row_with_refs(table, row, sep, refs));
    }
    errors
}

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

pub fn validate_enum(enum_def: &EnumDef) -> Vec<ValidationError> {
    let mut errors = validate_enum_schema(enum_def);
    for row in 0..enum_def.entries.len() {
        errors.extend(validate_enum_row(enum_def, row));
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
}
