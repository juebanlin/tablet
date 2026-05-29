use crate::model::*;
use crate::types::{TblType, Paradigm, SeparatorsSection};

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

/// 校验 @Xxx 字段类型本身（schema 层，不看具体值）
pub fn validate_ref_type(type_str: &str, refs: &RefIndex) -> Option<String> {
    let t = TblType::parse(type_str)?;
    if t.paradigm != Paradigm::Ref { return None; }
    let name = t.ref_name.as_deref()?;
    match refs.lookup(name) {
        None => Some(format!("引用的配置项 {} 不存在", name)),
        Some(RefKind::Constant) => Some("不能引用 constant（无 id 概念）".to_string()),
        _ => None,
    }
}

/// 校验 @Xxx 字段的具体值是否能在被引用项中找到
fn validate_ref_value(type_str: &str, value: &str, refs: &RefIndex) -> Option<String> {
    if value.is_empty() { return None; }
    let t = TblType::parse(type_str)?;
    if t.paradigm != Paradigm::Ref { return None; }
    let name = t.ref_name.as_deref()?;
    match refs.lookup(name) {
        None => Some(format!("引用的配置项 {} 不存在", name)),
        Some(RefKind::Constant) => Some("不能引用 constant".to_string()),
        Some(_) => {
            if refs.id_exists(name, value) { None }
            else { Some(format!("引用值 {} 不存在于 {}", value, name)) }
        }
    }
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

pub fn validate_table_cell(table: &Table, row: usize, col: usize, sep: &SeparatorsSection) -> Option<String> {
    validate_table_cell_with_refs(table, row, col, sep, None)
}

pub fn validate_table_cell_with_refs(
    table: &Table,
    row: usize,
    col: usize,
    sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
) -> Option<String> {
    let fields = &table.schema.fields;
    if col >= fields.len() { return None; }
    let value = table.records.get(row).and_then(|r| r.get(col)).map(|s| s.as_str()).unwrap_or("");
    if value.is_empty() { return None; }

    if fields[col].name == "id" {
        if value.parse::<i64>().is_err() {
            return Some("ID必须是数字".to_string());
        }
    }

    let type_str = &fields[col].tbl_type;
    if let Some(tbl_type) = TblType::parse(type_str) {
        if let Some(msg) = tbl_type.validate_value(value, sep) {
            return Some(msg);
        }
        if let Some(refs) = refs {
            if let Some(msg) = validate_ref_value(type_str, value, refs) {
                return Some(msg);
            }
        }
    }
    None
}

pub fn validate_table_row(table: &Table, row: usize, sep: &SeparatorsSection) -> Vec<(usize, String)> {
    validate_table_row_with_refs(table, row, sep, None)
}

pub fn validate_table_row_with_refs(
    table: &Table,
    row: usize,
    sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
) -> Vec<(usize, String)> {
    let mut errors = Vec::new();
    let fields = &table.schema.fields;
    let record = match table.records.get(row) { Some(r) => r, None => return errors };

    for col in 0..fields.len() {
        if let Some(msg) = validate_table_cell_with_refs(table, row, col, sep, refs) {
            errors.push((col, msg));
        }
    }

    let index_col = fields.iter().position(|f| f.name == "id");
    if let Some(idx_col) = index_col {
        let id_val = record.get(idx_col).map(|s| s.as_str()).unwrap_or("");
        if id_val.is_empty() {
            let has_data = record.iter().enumerate().any(|(i, v)| i != idx_col && !v.is_empty());
            if has_data {
                errors.push((idx_col, "有数据但ID为空".to_string()));
            }
        }
    }
    errors
}

pub fn validate_constant_cell(entry: &ConstEntry, col: usize, sep: &SeparatorsSection) -> Option<String> {
    match col {
        0 => {
            let name = &entry.name;
            if name.is_empty() { return None; }
            validate_const_name(name)
        }
        2 => {
            let value = &entry.value;
            if value.is_empty() { return None; }
            if let Some(tbl_type) = TblType::parse(&entry.tbl_type) {
                return tbl_type.validate_value(value, sep);
            }
            None
        }
        _ => None,
    }
}

pub fn validate_constant_row(constant: &Constant, row: usize, sep: &SeparatorsSection) -> Vec<(usize, String)> {
    let mut errors = Vec::new();
    let entry = match constant.entries.get(row) { Some(e) => e, None => return errors };

    for col in 0..5 {
        if let Some(msg) = validate_constant_cell(entry, col, sep) {
            errors.push((col, msg));
        }
    }

    if !entry.name.is_empty() && entry.value.is_empty() {
        errors.push((2, "name已填但value为空".to_string()));
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

pub fn validate_enum_entry_name(name: &str) -> Option<String> {
    if name.is_empty() { return Some("枚举条目名不能为空".to_string()); }
    if !is_valid_enum_entry_name(name) {
        return Some(format!("\"{}\" 不是合法枚举条目名（需大写字母开头，只含大写字母/数字/下划线）", name));
    }
    if is_reserved_keyword(name) {
        return Some(format!("\"{}\" 是语言关键字", name));
    }
    None
}

pub fn validate_enum_id(id: &str) -> Option<String> {
    if id.is_empty() { return None; }
    match id.parse::<i32>() {
        Ok(v) if v < 0 => Some("id 必须是正整数".to_string()),
        Ok(0) => Some("id 不能为 0（保留为未设置语义）".to_string()),
        Ok(_) => None,
        Err(_) => Some("id 必须是正整数".to_string()),
    }
}

pub fn validate_enum_cell(entry: &EnumEntry, col: usize) -> Option<String> {
    match col {
        0 => validate_enum_id(&entry.id),
        1 => {
            if entry.name.is_empty() { return None; }
            validate_enum_entry_name(&entry.name)
        }
        _ => None,
    }
}

pub fn validate_enum_row(enum_def: &EnumDef, row: usize) -> Vec<(usize, String)> {
    let mut errors = Vec::new();
    let entry = match enum_def.entries.get(row) { Some(e) => e, None => return errors };

    for col in 0..3 {
        if let Some(msg) = validate_enum_cell(entry, col) {
            errors.push((col, msg));
        }
    }

    if !entry.name.is_empty() && entry.id.is_empty() {
        errors.push((0, "name已填但id为空".to_string()));
    }
    errors
}

// --- 字段名验证 ---

pub fn validate_field_name(name: &str) -> Option<String> {
    if name.is_empty() { return Some("字段名不能为空".to_string()); }
    if !is_valid_identifier(name) {
        return Some(format!("\"{}\" 不是合法字段名（需小写字母开头，只含小写字母/数字/下划线）", name));
    }
    if is_reserved_keyword(name) {
        return Some(format!("\"{}\" 是语言关键字", name));
    }
    None
}

pub fn validate_const_name(name: &str) -> Option<String> {
    if name.is_empty() { return None; }
    if !is_valid_identifier(name) {
        return Some(format!("\"{}\" 不是合法常量名（需小写字母开头，只含小写字母/数字/下划线）", name));
    }
    if is_reserved_keyword(name) {
        return Some(format!("\"{}\" 是语言关键字", name));
    }
    None
}

// --- 表头验证 ---

#[derive(Debug, Clone)]
pub struct SchemaError {
    pub field: String,
    pub message: String,
}

pub fn validate_table_schema(table: &Table, sep: &SeparatorsSection) -> Vec<SchemaError> {
    validate_table_schema_with_refs(table, sep, None)
}

pub fn validate_table_schema_with_refs(
    table: &Table,
    _sep: &SeparatorsSection,
    refs: Option<&RefIndex>,
) -> Vec<SchemaError> {
    let mut errors = Vec::new();
    let fields = &table.schema.fields;

    if fields.is_empty() {
        errors.push(SchemaError { field: String::new(), message: "表没有定义任何字段".to_string() });
        return errors;
    }

    if fields[0].name != "id" {
        errors.push(SchemaError {
            field: fields[0].name.clone(),
            message: "第一列必须是主键 id".to_string(),
        });
    }

    let mut seen_names = std::collections::HashSet::new();
    for field in fields {
        if let Some(msg) = validate_field_name(&field.name) {
            errors.push(SchemaError { field: field.name.clone(), message: msg });
        }
        if !field.name.is_empty() && !seen_names.insert(&field.name) {
            errors.push(SchemaError {
                field: field.name.clone(),
                message: format!("字段名 \"{}\" 重复", field.name),
            });
        }
        if TblType::parse(&field.tbl_type).is_none() {
            errors.push(SchemaError {
                field: field.name.clone(),
                message: format!("类型 \"{}\" 不合法", field.tbl_type),
            });
        } else if let Some(refs) = refs {
            if let Some(msg) = validate_ref_type(&field.tbl_type, refs) {
                errors.push(SchemaError { field: field.name.clone(), message: msg });
            }
        }
    }

    // 主键值重复检测
    if let Some(idx_col) = fields.iter().position(|f| f.name == "id") {
        let mut seen_ids = std::collections::HashSet::new();
        for (row, record) in table.records.iter().enumerate() {
            let id = record.get(idx_col).map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() { continue; }
            if !seen_ids.insert(id.to_string()) {
                errors.push(SchemaError {
                    field: "id".to_string(),
                    message: format!("第{}行主键值 \"{}\" 重复", row + 1, id),
                });
            }
        }
    }

    errors
}

pub fn validate_constant_schema(constant: &Constant, _sep: &SeparatorsSection) -> Vec<SchemaError> {
    let mut errors = Vec::new();

    let mut seen_names = std::collections::HashSet::new();
    for entry in &constant.entries {
        if entry.name.is_empty() { continue; }

        if let Some(msg) = validate_const_name(&entry.name) {
            errors.push(SchemaError { field: entry.name.clone(), message: msg });
        }
        if !seen_names.insert(&entry.name) {
            errors.push(SchemaError {
                field: entry.name.clone(),
                message: format!("常量名 \"{}\" 重复", entry.name),
            });
        }
        match TblType::parse(&entry.tbl_type) {
            None => errors.push(SchemaError {
                field: entry.name.clone(),
                message: format!("类型 \"{}\" 不合法", entry.tbl_type),
            }),
            Some(t) if t.paradigm == Paradigm::Ref => errors.push(SchemaError {
                field: entry.name.clone(),
                message: "constant 不允许使用 @Xxx 引用类型".to_string(),
            }),
            _ => {}
        }
    }

    errors
}

pub fn validate_enum_schema(enum_def: &EnumDef) -> Vec<SchemaError> {
    let mut errors = Vec::new();

    if enum_def.entries.iter().all(|e| e.id.is_empty() && e.name.is_empty()) {
        errors.push(SchemaError { field: String::new(), message: "枚举至少需要一个条目".to_string() });
        return errors;
    }

    let mut seen_ids = std::collections::HashSet::new();
    let mut seen_names = std::collections::HashSet::new();
    for entry in &enum_def.entries {
        if entry.id.is_empty() && entry.name.is_empty() { continue; }
        if let Some(msg) = validate_enum_id(&entry.id) {
            errors.push(SchemaError { field: entry.name.clone(), message: msg });
        }
        if let Some(msg) = validate_enum_entry_name(&entry.name) {
            errors.push(SchemaError { field: entry.name.clone(), message: msg });
        }
        if !entry.id.is_empty() && !seen_ids.insert(entry.id.clone()) {
            errors.push(SchemaError {
                field: entry.name.clone(),
                message: format!("id \"{}\" 重复", entry.id),
            });
        }
        if !entry.name.is_empty() && !seen_names.insert(entry.name.clone()) {
            errors.push(SchemaError {
                field: entry.name.clone(),
                message: format!("枚举条目名 \"{}\" 重复", entry.name),
            });
        }
    }

    errors
}
