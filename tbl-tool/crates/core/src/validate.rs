use crate::model::*;
use crate::types::{TblType, SeparatorsSection};

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
    let fields = &table.schema.fields;
    if col >= fields.len() { return None; }
    let value = table.records.get(row).and_then(|r| r.get(col)).map(|s| s.as_str()).unwrap_or("");
    if value.is_empty() { return None; }

    if fields[col].name == table.schema.index {
        if value.parse::<i64>().is_err() {
            return Some("ID必须是数字".to_string());
        }
    }

    let type_str = &fields[col].tbl_type;
    if let Some(tbl_type) = TblType::parse(type_str) {
        return tbl_type.validate_value(value, sep);
    }
    None
}

pub fn validate_table_row(table: &Table, row: usize, sep: &SeparatorsSection) -> Vec<(usize, String)> {
    let mut errors = Vec::new();
    let fields = &table.schema.fields;
    let record = match table.records.get(row) { Some(r) => r, None => return errors };

    for col in 0..fields.len() {
        if let Some(msg) = validate_table_cell(table, row, col, sep) {
            errors.push((col, msg));
        }
    }

    let index_col = fields.iter().position(|f| f.name == table.schema.index);
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
    let mut errors = Vec::new();
    let fields = &table.schema.fields;

    if fields.is_empty() {
        errors.push(SchemaError { field: String::new(), message: "表没有定义任何字段".to_string() });
        return errors;
    }

    let index_exists = fields.iter().any(|f| f.name == table.schema.index);
    if !index_exists {
        errors.push(SchemaError {
            field: table.schema.index.clone(),
            message: format!("主键 \"{}\" 在字段列表中不存在", table.schema.index),
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
        }
    }

    // 主键值重复检测
    if let Some(idx_col) = fields.iter().position(|f| f.name == table.schema.index) {
        let mut seen_ids = std::collections::HashSet::new();
        for (row, record) in table.records.iter().enumerate() {
            let id = record.get(idx_col).map(|s| s.as_str()).unwrap_or("");
            if id.is_empty() { continue; }
            if !seen_ids.insert(id.to_string()) {
                errors.push(SchemaError {
                    field: table.schema.index.clone(),
                    message: format!("第{}行主键值 \"{}\" 重复", row + 1, id),
                });
            }
        }
    }

    errors
}

pub fn validate_constant_schema(constant: &Constant, sep: &SeparatorsSection) -> Vec<SchemaError> {
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
        if TblType::parse(&entry.tbl_type).is_none() {
            errors.push(SchemaError {
                field: entry.name.clone(),
                message: format!("类型 \"{}\" 不合法", entry.tbl_type),
            });
        }
    }

    errors
}
