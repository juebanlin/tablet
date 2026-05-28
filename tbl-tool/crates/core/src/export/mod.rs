mod json;
mod java;

pub use json::export_all_json;
pub use java::export_all_java;

use serde_json::Value;
use crate::types::BaseType;

pub enum EmptyStrategy {
    Null,
    Omit,
}

impl EmptyStrategy {
    pub fn from_config(s: &str) -> Self {
        match s {
            "omit" => Self::Omit,
            _ => Self::Null,
        }
    }
}

pub enum LineEnding {
    Lf,
    Crlf,
}

impl LineEnding {
    pub fn from_config(s: &str) -> Self {
        match s {
            "crlf" => Self::Crlf,
            _ => Self::Lf,
        }
    }

    pub fn normalize(&self, content: &str) -> String {
        let lf = content.replace("\r\n", "\n");
        match self {
            Self::Lf => lf,
            Self::Crlf => lf.replace('\n', "\r\n"),
        }
    }
}

pub fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut upper_next = false;
    for ch in s.chars() {
        if ch == '_' {
            upper_next = true;
        } else if upper_next {
            result.push(ch.to_ascii_uppercase());
            upper_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

pub fn to_pascal_case(s: &str) -> String {
    let camel = to_camel_case(s);
    let mut chars = camel.chars();
    match chars.next() {
        Some(c) => c.to_ascii_uppercase().to_string() + chars.as_str(),
        None => String::new(),
    }
}

pub fn parse_base_value(raw: &str, bt: &BaseType) -> Value {
    match bt {
        BaseType::Int | BaseType::Long => {
            raw.parse::<i64>().map(Value::from).unwrap_or(Value::from(0))
        }
        BaseType::Float | BaseType::Double => {
            raw.parse::<f64>().map(Value::from).unwrap_or(Value::from(0.0))
        }
        BaseType::Bool => Value::from(raw == "true" || raw == "1"),
        BaseType::Str => Value::from(raw),
    }
}
