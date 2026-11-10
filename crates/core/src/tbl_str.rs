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

use crate::types::{BaseType, TblType, TypeParam};

const SPECIAL_CHARS: &[char] = &['\\', '|', '\n', '\r', '\t'];

/// 判断 tbl_type 是否需要走转义。
/// 规则：解析类型字符串，若任何参数是 BaseType::Txt → 走转义。
/// 解析失败 → 兜底走转义（安全第一）。
fn needs_escape(tbl_type: &str) -> bool {
    TblType::parse(tbl_type)
        .map(|tt| tt.params.iter().any(|p| matches!(p, TypeParam::Base(BaseType::Txt))))
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
        // `|` 是 v2 的列分隔符，普通字段 / 自定义容器分隔符里出现它是正常的，
        // encode 负责转义成 `\|`；这里不把它当成异常。其它特殊字符仍需告警。
        if value.contains(['\\', '\n', '\r', '\t']) {
            eprintln!(
                "[tbl_str] WARN: 非 txt 字段({})含特殊字符，防御转义: {:?}",
                tbl_type, value
            );
            return encode_str(value);
        }
        if value.contains('|') {
            return encode_str(value);
        }
        value.to_string()
    }
}

/// v3（`字段名:值` 行）的单元格编码：**竖线不转义**。
///
/// v3 一行的结构是 `字段名:值`，值里没有「列分隔符」这回事；`|` 只是普通字符。
/// 仍按 v2 的规则转义它，会让加载后原样保存（哪怕只改了别的格子）凭空多出 `\|`，
/// git 上表现为某个没碰过的列变了。`\` 与换行 / 制表符仍必须转义，否则值会破坏
/// 行结构或与转义序列混淆。
pub fn encode_v3(value: &str) -> String {
    if !value.contains(SPECIAL_CHARS) {
        return value.to_string();
    }
    let mut out = String::with_capacity(value.len() + 8);
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
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
            if !only_pipe_escapes(value) {
                eprintln!(
                    "[tbl_str] WARN: 非 txt 字段({})含反斜杠，防御反转义: {:?}",
                    tbl_type, value
                );
            }
            return decode_str(value);
        }
        value.to_string()
    }
}

/// 非 txt 字段里 `\|` 是 v2 行分隔符的合法转义（自定义 List/Map 分隔符可能就用 `|`），
/// 不应该当成数据异常告警；其它反斜杠转义仍然告警。
fn only_pipe_escapes(value: &str) -> bool {
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            match chars.next() {
                Some('|') => {}
                _ => return false,
            }
        }
    }
    true
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

    /// v3 的 `字段名:值` 行没有列分隔符概念，竖线必须原样保留；
    /// 反斜杠 / 换行仍转义，否则值会破坏行结构或与转义序列混淆。
    #[test]
    fn v3_encoding_keeps_pipe_but_escapes_others() {
        assert_eq!(encode_v3("property:1001#100|1002#10"), "property:1001#100|1002#10");
        assert_eq!(encode_v3("a\\b|c\nd"), "a\\\\b|c\\nd");
        let raw = "path\\to|file\nline2\tend";
        assert_eq!(decode(&encode_v3(raw), "txt"), raw);
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
    fn non_txt_pipe_escape_is_not_an_error() {
        // 自定义 List/Map 分隔符可能用 `|`；v2 行分隔符必须把它写成 `\\|`。
        assert_eq!(encode("a|b", "List<List<int>>"), "a\\|b");
        assert_eq!(decode("a\\|b", "List<List<int>>"), "a|b");
        assert!(only_pipe_escapes("a\\|b"));
        assert!(only_pipe_escapes("\\|\\|"));
        assert!(!only_pipe_escapes("a\\\\b"));
        assert!(!only_pipe_escapes("a\\nb"));
        assert!(!only_pipe_escapes("a\\"));
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
