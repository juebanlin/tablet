//! 配置叶子的通用读/写小工具。`ExportKey` 与 `UiKey` 两个 key 空间共用一份。
//!
//! 抽出来是因为两边都要**同样严**的解析：布尔只认 `true` / `false`，枚举认不出就
//! **保持原样**而不是清空。这类规则分散成两份迟早会漂，而漂了不会编译报错 —— 正是
//! `ExportKey` 当初要消灭的那种问题。
//!
//! 每种解析各两套：`Option<T>` 叶子（缺省 = 不设）与裸 `T` 叶子。

use std::str::FromStr;

pub fn bool_str(b: bool) -> String {
    if b { "true".to_string() } else { "false".to_string() }
}

/// 空串 = 不设（`None`），语义是「不覆盖，用上一层配置的值」。
pub fn opt_string(v: &str) -> Option<String> {
    if v.is_empty() { None } else { Some(v.to_string()) }
}

/// 严格布尔：只认 `true` / `false`（大小写不敏感）。
/// 比 `v == "true"` 严 —— 那个写法随便什么串都会静默变成 false。
pub fn parse_bool(v: &str) -> Option<bool> {
    match v.trim().to_ascii_lowercase().as_str() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// 解析失败一律**保持原样**并返回 false（别改成清空 —— 那会把一个笔误变成一次删除）。
pub fn set_bool(slot: &mut Option<bool>, v: &str) -> bool {
    match parse_bool(v) {
        Some(b) => { *slot = Some(b); true }
        None => false,
    }
}

pub fn set_plain_bool(slot: &mut bool, v: &str) -> bool {
    match parse_bool(v) {
        Some(b) => { *slot = b; true }
        None => false,
    }
}

pub fn set_enum<T: FromStr>(slot: &mut Option<T>, v: &str) -> bool {
    match T::from_str(v.trim()) {
        Ok(e) => { *slot = Some(e); true }
        Err(_) => false,
    }
}

pub fn set_plain_enum<T: FromStr>(slot: &mut T, v: &str) -> bool {
    match T::from_str(v.trim()) {
        Ok(e) => { *slot = e; true }
        Err(_) => false,
    }
}
