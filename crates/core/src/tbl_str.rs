//! .tbl 单元格字符串编解码。
//!
//! 独立模块，与解析器/验证器解耦。
//!
//! ## 分层关系
//!
//! 验证层 (types.rs) →  save 前拦截不合规值（str 不能含 \ | \n 等）
//! 编解码层 (本模块) →  裸值 ↔ .tbl 存储表示
//! 格式层 (tbl.rs)   →  文件 I/O，调用 encode/decode
//!
//! ## encode/decode 策略
//!
//! 通过 `TblType::parse(tbl_type)` 判断是否需要转义：
//! - 参数含 BaseType::Txt → 走 encode_str / decode_str（转义 5 个特殊字符）
//! - 参数不含 Txt         → 原样直写，但含特殊字符时防御转义 + stderr 告警
//!
//! 防御性 fallback：即使验证层漏了或手改文件引入特殊字符，encode/decode
//! 也会兜底保证 .tbl 格式安全，同时 stderr 告警暴露问题。

use crate::types::{BaseType, TblType};

const SPECIAL_CHARS: &[char] = &['\\', '|', '\n', '\r', '\t'];

/// 判断 tbl_type 是否需要走转义。
/// 规则：解析类型字符串，若任何参数是 BaseType::Txt → 走转义。
/// 解析失败 → 兜底走转义（安全第一）。
fn needs_escape(tbl_type: &str) -> bool {
    TblType::parse(tbl_type)
        .map(|tt| tt.params.iter().any(|bt| *bt == BaseType::Txt))
        .unwrap_or(true)
}

/// 内存字符串 → .tbl 存储表示。
///
/// 参数含 Txt → 转义 `\` `|` `\n` `\r` `\t`
/// 参数不含 Txt → 原样返回；若意外含特殊字符则防御转义 + stderr warn
pub fn encode(value: &str, tbl_type: &str) -> String {
    if needs_escape(tbl_type) {
        encode_str(value)
    } else {
        if value.contains(SPECIAL_CHARS) {
            eprintln!(
                "[tbl_str] WARN: 非 txt 字段({})含特殊字符，防御转义: {:?}",
                tbl_type, value
            );
            return encode_str(value);
        }
        value.to_string()
    }
}

/// .tbl 存储表示 → 内存字符串。
///
/// 参数含 Txt → 反向解析转义序列
/// 参数不含 Txt → 原样返回；若意外含 `\` 则防御反转义 + stderr warn
pub fn decode(value: &str, tbl_type: &str) -> String {
    if needs_escape(tbl_type) {
        decode_str(value)
    } else {
        if value.contains('\\') {
            eprintln!(
                "[tbl_str] WARN: 非 txt 字段({})含反斜杠，防御反转义: {:?}",
                tbl_type, value
            );
            return decode_str(value);
        }
        value.to_string()
    }
}

/// 转义实现（内部）。5 个特殊字符：\ | \n \r \t
fn encode_str(s: &str) -> String {
    if !s.contains(SPECIAL_CHARS) {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '|'  => out.push_str("\\|"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

/// 反转义实现（内部）。未知转义序列保留原样两字符。
fn decode_str(s: &str) -> String {
    if !s.contains('\\') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        match chars.next() {
            Some('\\') => out.push('\\'),
            Some('|')  => out.push('|'),
            Some('n')  => out.push('\n'),
            Some('r')  => out.push('\r'),
            Some('t')  => out.push('\t'),
            Some(other) => { out.push('\\'); out.push(other); }
            None => out.push('\\'),
        }
    }
    out
}

/// 拆一行为字段（考虑转义，仅 v2 格式使用）。
///
/// 返回的每个字段仍是编码状态，调用方按需 decode。
pub fn split_row(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            cur.push(ch);
            if let Some(&nx) = chars.peek() {
                cur.push(nx);
                chars.next();
            }
        } else if ch == '|' {
            out.push(std::mem::take(&mut cur));
        } else {
            cur.push(ch);
        }
    }
    out.push(cur);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn needs_escape_txt() {
        assert!(needs_escape("txt"));
        // non-txt
        assert!(!needs_escape("str"));
        assert!(!needs_escape("int"));
        assert!(!needs_escape("bool"));
        assert!(!needs_escape("List<str>"));
        assert!(!needs_escape("Map<int,str>"));
        assert!(!needs_escape("Tuple2<str,int>"));
        // invalid → safe fallback
        assert!(needs_escape("unknown"));
    }

    #[test]
    fn text_round_trip() {
        let cases = ["hello", "中文", "a|b", "line1\nline2", "col1\tcol2",
            "path\\to\\file", r#"<div>Hi</div>"#, "", "\\", "||"];
        for s in cases {
            let enc = encode(s, "txt");
            let dec = decode(&enc, "txt");
            assert_eq!(dec, s, "txt round-trip failed for: {:?}", s);
        }
    }

    #[test]
    fn atom_passthrough_clean() {
        for s in ["42", "true", "3.14", "1;2;3", "1,2", "hello", "中文"] {
            assert_eq!(encode(s, "int"), s);
            assert_eq!(decode(s, "int"), s);
            assert_eq!(encode(s, "str"), s);
            assert_eq!(decode(s, "str"), s);
            assert_eq!(encode(s, "List<str>"), s);
            assert_eq!(decode(s, "List<str>"), s);
        }
    }

    #[test]
    fn txt_encoding_specific() {
        assert_eq!(encode("a|b", "txt"), "a\\|b");
        assert_eq!(encode("a\nb", "txt"), "a\\nb");
        assert_eq!(encode("a\\b", "txt"), "a\\\\b");
    }

    #[test]
    fn split_row_respects_escape() {
        let r = split_row("a\\|b|c");
        assert_eq!(r, vec!["a\\|b", "c"]);
    }

    #[test]
    fn split_row_empty_fields() {
        let r = split_row("a||b");
        assert_eq!(r, vec!["a", "", "b"]);
    }

    #[test]
    fn fast_path_no_special_chars() {
        let s = "hello 中文 <div>";
        assert_eq!(encode(s, "txt"), s);
        assert_eq!(decode(s, "txt"), s);
        assert_eq!(encode(s, "int"), s);
        assert_eq!(decode(s, "int"), s);
    }
}
