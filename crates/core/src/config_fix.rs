//! 配置文件加载与自动修复。
//!
//! 读取 tablet.toml 时，对枚举类型字段进行校验：
//! - 无效值 → 重置为默认值
//! - 空字符串 → 重置为默认值
//! 发现任何修复后立即写回磁盘，返回 (GlobalConfig, bool) 表示是否修复过。

use crate::model::GlobalConfig;
use anyhow::Result;
use std::path::Path;

/// 加载并修复全局配置。返回 (config, fixed)，fixed=true 表示发现并修复了无效值。
pub fn load_and_fix_global_config(config_path: &Path) -> Result<(GlobalConfig, bool)> {
    let text = std::fs::read_to_string(config_path)?;

    // 先检查文本中是否有无效值（即使 serde 能用 default 绕过）
    let mut fixed_text = text.clone();
    let mut fixed = false;

    // 修复 project_sort = "bad_value" -> "id"
    if let Some(new_text) = fix_enum_field(&fixed_text, "project_sort", &["id", "name", "open", "created", "manual"], "id") {
        fixed_text = new_text;
        fixed = true;
    }

    // 修复 log_level = "invalid_level" -> "debug"
    if let Some(new_text) = fix_enum_field(&fixed_text, "log_level", &["debug", "info", "warn", "error"], "debug") {
        fixed_text = new_text;
        fixed = true;
    }

    // 修复 picker_trigger_header = "" -> "single"
    if let Some(new_text) = fix_empty_or_invalid_field(&fixed_text, "picker_trigger_header", &["single", "double"], "single") {
        fixed_text = new_text;
        fixed = true;
    }

    // 修复 picker_trigger_data = "" -> "double"
    if let Some(new_text) = fix_enum_field(&fixed_text, "picker_trigger_data", &["single", "double"], "double") {
        fixed_text = new_text;
        fixed = true;
    }

    // 修复 encoding = "unknown-encoding" -> "utf-8"
    if let Some(new_text) = fix_enum_field(&fixed_text, "encoding", &["utf-8", "gbk", "big5"], "utf-8") {
        fixed_text = new_text;
        fixed = true;
    }

    // 修复 line_ending = "bad" -> "lf"
    if let Some(new_text) = fix_enum_field(&fixed_text, "line_ending", &["lf", "crlf"], "lf") {
        fixed_text = new_text;
        fixed = true;
    }

    // 解析（使用修复后的文本或原文本）
    let config: GlobalConfig = toml::from_str(&fixed_text)?;

    // 如果修复过，立即写回磁盘
    if fixed {
        std::fs::write(config_path, &fixed_text)?;
    }

    Ok((config, fixed))
}

/// 修复空字符串或无效枚举值的字段。
fn fix_empty_or_invalid_field(text: &str, field_name: &str, valid_values: &[&str], default_value: &str) -> Option<String> {
    let mut fixed = false;
    let mut lines: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();

        // 查找字段行：field_name = "value" 或 field_name = ""
        if trimmed.starts_with(field_name) {
            let after = trimmed[field_name.len()..].trim_start();
            if after.starts_with('=') {
                let value_part = after[1..].trim();

                // 检查是否是空字符串 ""
                if value_part == "\"\"" {
                    let indent = line.len() - line.trim_start().len();
                    let new_line = format!("{}{} = \"{}\"", " ".repeat(indent), field_name, default_value);
                    lines.push(new_line);
                    fixed = true;
                    continue;
                }

                // 否则检查是否是无效值
                if let Some(quoted_value) = extract_quoted_value(value_part) {
                    if !valid_values.contains(&quoted_value.as_str()) {
                        let indent = line.len() - line.trim_start().len();
                        let new_line = format!("{}{} = \"{}\"", " ".repeat(indent), field_name, default_value);
                        lines.push(new_line);
                        fixed = true;
                        continue;
                    }
                }
            }
        }

        lines.push(line.to_string());
    }

    if fixed {
        Some(lines.join("\n"))
    } else {
        None
    }
}

/// 修复 TOML 文本中的枚举字段：如果值无效，替换为默认值。
/// 返回修复后的文本，如果没有修复则返回 None。
fn fix_enum_field(text: &str, field_name: &str, valid_values: &[&str], default_value: &str) -> Option<String> {
    let mut fixed = false;
    let mut lines: Vec<String> = Vec::new();

    for line in text.lines() {
        let trimmed = line.trim();

        // 查找字段行：field_name = "value"
        if trimmed.starts_with(field_name) {
            let after = trimmed[field_name.len()..].trim_start();
            if after.starts_with('=') {
                // 提取引号内的值
                let value_part = after[1..].trim();
                if let Some(quoted_value) = extract_quoted_value(value_part) {
                    // 检查是否是有效值
                    if !valid_values.contains(&quoted_value.as_str()) {
                        // 无效值，替换为默认值
                        let indent = line.len() - line.trim_start().len();
                        let new_line = format!("{}{} = \"{}\"", " ".repeat(indent), field_name, default_value);
                        lines.push(new_line);
                        fixed = true;
                        continue;
                    }
                }
            }
        }

        lines.push(line.to_string());
    }

    if fixed {
        Some(lines.join("\n"))
    } else {
        None
    }
}

/// 从 TOML 值部分提取引号内的字符串（简化版，不处理转义）。
fn extract_quoted_value(value_part: &str) -> Option<String> {
    let trimmed = value_part.trim();
    if trimmed.starts_with('"') {
        if let Some(end_quote) = trimmed[1..].find('"') {
            return Some(trimmed[1..1 + end_quote].to_string());
        }
    }
    None
}
