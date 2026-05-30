// 名字搜索匹配：字面子串 + 拼音首字母子串。
// 用途：树面板、类型选择器、引用选择器等"按名字过滤"场景共用同一份匹配规则。
//
// 规则（query / name 都先 lowercase）：
// 1. name.contains(query)             - 字面子串
// 2. pinyin_initials(name).contains(query) - 汉字逐字取首字母，非汉字字符原样保留
// 任一命中即视作匹配；空 query 永远命中。
//
// 例：
//   "英雄表"    → 拼音首字母串 "yxb"      ⇒ 输 "yxb" / "YXB" / "xb" 都命中
//   "HMM英雄表" → 拼音首字母串 "hmmyxb"   ⇒ 输 "hmmyxb" / "yxb" 都命中
//   "new英雄表" → 拼音首字母串 "newyxb"   ⇒ 输 "newyxb" / "yxb" 都命中
//   "HeroBase" → 拼音首字母串 "herobase" ⇒ 输 "hb" 不命中（contains 不是子序列）
//
// 不实现拼音全拼（"yingxiong" 命中"英雄"），按当前需求首字母够用。

use pinyin::ToPinyin;

/// name 是否被 query 命中。空 query 视作命中。匹配规则见模块注释。
pub fn name_matches(name: &str, query: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let q = query.to_lowercase();
    let n = name.to_lowercase();
    if n.contains(&q) {
        return true;
    }
    let initials = pinyin_initials(name);
    initials.contains(&q)
}

/// 把字符串转成"拼音首字母 + 非汉字原样"的小写串。
/// 汉字 → 取该字 pinyin 的首字符；非汉字（ASCII / emoji / 标点 / 空格）原样保留。
fn pinyin_initials(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch.to_pinyin() {
            Some(p) => {
                // p.plain() 形如 "ying"；取首字符即可
                if let Some(c) = p.plain().chars().next() {
                    out.push(c);
                }
            }
            None => {
                for c in ch.to_lowercase() {
                    out.push(c);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_matches_anything() {
        assert!(name_matches("HeroBase", ""));
        assert!(name_matches("英雄", ""));
        assert!(name_matches("", ""));
    }

    #[test]
    fn literal_substring() {
        assert!(name_matches("HeroBase", "hero"));
        assert!(name_matches("HeroBase", "ERO"));
        assert!(name_matches("HeroBase", "Base"));
    }

    #[test]
    fn chinese_initials() {
        assert!(name_matches("英雄表", "yxb"));
        assert!(name_matches("英雄表", "YXB"));
        assert!(name_matches("英雄表", "xb"));
        assert!(name_matches("英雄", "yx"));
    }

    #[test]
    fn mixed_chinese_and_english() {
        assert!(name_matches("HMM英雄表", "hmmyxb"));
        assert!(name_matches("HMM英雄表", "mmyxb"));
        assert!(name_matches("HMM英雄表", "yxb"));
        assert!(name_matches("new英雄表", "newyxb"));
        assert!(name_matches("new英雄表", "newy"));
        assert!(name_matches("new英雄表", "wyx"));
    }

    #[test]
    fn full_pinyin_not_supported() {
        // 仅首字母，不实现全拼匹配
        assert!(!name_matches("英雄表", "yingxiong"));
    }

    #[test]
    fn miss_cases() {
        assert!(!name_matches("HeroBase", "yxb"));
        assert!(!name_matches("英雄表", "abc"));
        // contains 不是子序列匹配：HeroBase → "herobase" 不含 "hb"
        assert!(!name_matches("HeroBase", "hb"));
    }
}
