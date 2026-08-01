//! .tbl 单元格字符串编解码（黑盒接口）
//!
//! 独立模块，与解析器解耦。
//!
//! Text / Atom 两类字段统一走转义/反转义，保证 round-trip 安全。
//! 业务层由 types.rs 的 validate_str / validate_base 负责拦截不合规值；
//! encode/decode 不参与业务判断，只保证格式安全。

/// 字段类别 — 当前两类走相同路径，保留枚举用于语义标记。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    /// 自由文本字段（txt 类型）。
    Text,
    /// 简单值字段（int/long/float/double/bool/str 等）。
    Atom,
}

/// 判断 tbl_type 字符串属于哪类字段。
///
/// 当前规则：`txt` → Text，其余全 → Atom（两类走相同编码路径，
/// 仅用于标注语义；实际行为无差别）。
pub fn classify(tbl_type: &str) -> FieldKind {
    if tbl_type == "txt" {
        FieldKind::Text
    } else {
        FieldKind::Atom
    }
}

/// 内存字符串 -> .tbl 存储表示。
///
/// 两类字段统一走转义，不 panic。str 字段用户可能输入反斜杠或管道符，
/// 在 save 时由 validate_str 拦截；encode 不 panic，保证 round-trip 安全。
pub fn encode(value: &str, _kind: FieldKind) -> String {
    encode_str(value)
}

/// .tbl 存储表示 -> 内存字符串。
///
/// 两类字段统一走反转义，与 encode 对称。
pub fn decode(value: &str, _kind: FieldKind) -> String {
    decode_str(value)
}

/// 转义实现（内部）。5 个特殊字符：\ | \n \r \t
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

/// 拆一行为字段（考虑转义）。
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
    fn classify_types() {
        assert_eq!(classify("txt"), FieldKind::Text);
        assert_eq!(classify("str"), FieldKind::Atom);
        assert_eq!(classify("List<str>"), FieldKind::Atom);
        assert_eq!(classify("Map<int,str>"), FieldKind::Atom);
        assert_eq!(classify("int"), FieldKind::Atom);
        assert_eq!(classify("bool"), FieldKind::Atom);
        assert_eq!(classify("List<int>"), FieldKind::Atom);
    }

    #[test]
    fn round_trip_all_kinds() {
        // Text 和 Atom 都走相同转义路径，都 round-trip 正确
        let cases = [
            "hello",
            "中文",
            "a|b",
            "line1\nline2",
            "col1\tcol2",
            "path\\to\\file",
            r#"<div class="box">Hi</div>"#,
            "",
            "\\",
            "||",
            "42",
            "true",
            "3.14",
            "1;2;3",
            "1,2",
        ];
        for s in cases {
            let enc = encode(s, FieldKind::Text);
            let dec = decode(&enc, FieldKind::Text);
            assert_eq!(dec, s, "Text round-trip failed for: {:?}", s);

            let enc = encode(s, FieldKind::Atom);
            let dec = decode(&enc, FieldKind::Atom);
            assert_eq!(dec, s, "Atom round-trip failed for: {:?}", s);
        }
    }

    #[test]
    fn encoding_specific_chars() {
        assert_eq!(encode("a|b", FieldKind::Text), "a\\|b");
        assert_eq!(encode("a\nb", FieldKind::Text), "a\\nb");
        assert_eq!(encode("a\\b", FieldKind::Text), "a\\\\b");
        // Atom 同样转义
        assert_eq!(encode("a|b", FieldKind::Atom), "a\\|b");
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
        assert_eq!(encode(s, FieldKind::Text), s);
        assert_eq!(decode(s, FieldKind::Text), s);
        assert_eq!(encode(s, FieldKind::Atom), s);
    }
}
