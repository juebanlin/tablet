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
            if name.contains(' ') { return Some("不能含空格".to_string()); }
            if !is_valid_identifier(name) { return Some("不是合法标识符".to_string()); }
            if is_java_keyword(name) || is_lua_keyword(name) { return Some("是语言关键字".to_string()); }
            None
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
    if !first.is_ascii_alphabetic() && first != '_' { return false; }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
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

pub fn is_reserved_keyword(name: &str) -> bool {
    is_java_keyword(name) || is_lua_keyword(name)
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
