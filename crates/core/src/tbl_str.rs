//! .tbl 单元格字符串编解码（黑盒接口）
//!
//! 独立模块，与解析器解耦。未来可以无痛切换实现（如改为 base64），
//! 只要保持这些函数的语义不变。
//!
//! 设计：按字段类型 case 处理。
//! - `Str` 类字段（str / List<str> / Map<K,str> 等含字符串的类型）：走完整转义
//! - `Atom` 类字段（int/long/float/bool 及其集合，纯数字/枚举值）：断言不含特殊字符
//!
//! `Atom` 类字段本应由业务层（validate/types）保证不含 `|`/`\n`；
//! 若出现则视为业务层 bug，编码时 panic 暴露而非静默通过。

/// 字段类别 — 决定 encode/decode 的策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// 字符串字段（str / List<str> / Map<K,str> 等含字符串内容）。
    /// 允许含任意字符，走完整转义。
    Str,
    /// 原子字段（int/long/float/bool 及其集合/元组/枚举 id 等）。
    /// 业务层保证不含 `|`/`\n`/`\r`/`\t`，encode 时断言。
    Atom,
}

/// 判断 tbl_type 字符串属于哪类字段。
///
/// 规则：类型字符串中出现 `str` 关键字 → `Str`；否则 `Atom`。
/// 例如：
/// - `str`, `List<str>`, `Map<int,str>`, `Tuple2<str,int>` → `Str`
/// - `int`, `bool`, `List<int>`, `Map<int,int>`, `Tuple3<int,int,int>` → `Atom`
pub fn classify(tbl_type: &str) -> FieldKind {
    if tbl_type.contains("str") {
        FieldKind::Str
    } else {
        FieldKind::Atom
    }
}

/// 内存字符串 → .tbl 存储表示。
///
/// - `Str` 字段：转义 5 个特殊字符（`\ | \n \r \t`）
/// - `Atom` 字段：断言不含特殊字符（业务层保证），原样返回；含则 panic
pub fn encode(value: &str, kind: FieldKind) -> String {
    match kind {
        FieldKind::Str => encode_str(value),
        FieldKind::Atom => {
            assert!(
                !value.contains(|c: char| matches!(c, '\\' | '|' | '\n' | '\r' | '\t')),
                "Atom field contains special char (business layer bug): {:?}", value
            );
            value.to_string()
        }
    }
}

/// .tbl 存储表示 → 内存字符串。
///
/// - `Str` 字段：反向解析转义序列
/// - `Atom` 字段：原样返回（不应含转义序列）
pub fn decode(value: &str, kind: FieldKind) -> String {
    match kind {
        FieldKind::Str => decode_str(value),
        FieldKind::Atom => value.to_string(),
    }
}

/// Str 字段的转义实现（内部）。
fn encode_str(s: &str) -> String {
    // fast-path: 无特殊字符直接返回
    if !s.contains(|c: char| matches!(c, '\\' | '|' | '\n' | '\r' | '\t')) {
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

/// Str 字段的反转义实现（内部）。未知转义序列（如 `\a`）保留原样两字符。
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

/// 拆一行为字段（考虑转义，`\|` 不作为分隔符）。
///
/// 返回的每个字段仍是**编码状态**，调用方按需 `decode`。
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
    fn classify_types() {
        assert_eq!(classify("str"), FieldKind::Str);
        assert_eq!(classify("List<str>"), FieldKind::Str);
        assert_eq!(classify("Map<int,str>"), FieldKind::Str);
        assert_eq!(classify("Tuple2<str,int>"), FieldKind::Str);
        assert_eq!(classify("int"), FieldKind::Atom);
        assert_eq!(classify("long"), FieldKind::Atom);
        assert_eq!(classify("bool"), FieldKind::Atom);
        assert_eq!(classify("float"), FieldKind::Atom);
        assert_eq!(classify("List<int>"), FieldKind::Atom);
        assert_eq!(classify("Map<int,int>"), FieldKind::Atom);
    }

    #[test]
    fn str_round_trip() {
        let cases = [
            "hello",
            "中文",
            "a|b",
            "line1\nline2",
            "col1\tcol2",
            "path\\to\\file",
            "<div class=\"box\">Hi</div>",
            "",
            "\\",
            "||",
        ];
        for s in cases {
            let enc = encode(s, FieldKind::Str);
            let dec = decode(&enc, FieldKind::Str);
            assert_eq!(dec, s, "round-trip failed for: {:?}", s);
        }
    }

    #[test]
    fn atom_pass_through() {
        for s in ["42", "true", "3.14", "1;2;3", "1,2", "1:a;2:b"] {
            assert_eq!(encode(s, FieldKind::Atom), s);
            assert_eq!(decode(s, FieldKind::Atom), s);
        }
    }

    #[test]
    #[should_panic(expected = "Atom field contains special char")]
    fn atom_panics_on_pipe() {
        // Atom 字段含 | 是业务层 bug，应 panic 暴露
        encode("1|2", FieldKind::Atom);
    }

    #[test]
    fn str_specific_encoding() {
        assert_eq!(encode("a|b", FieldKind::Str), "a\\|b");
        assert_eq!(encode("a\nb", FieldKind::Str), "a\\nb");
        assert_eq!(encode("a\\b", FieldKind::Str), "a\\\\b");
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
        // 无特殊字符时零替换返回
        let s = "hello 中文 <div>";
        assert_eq!(encode(s, FieldKind::Str), s);
        assert_eq!(decode(s, FieldKind::Str), s);
    }
}
