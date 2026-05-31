// 分隔符元数据：与 toml [separators] / SeparatorsSection 一一对应的 25 项 leaf。
// JSON / XML 都从这里取，避免两端清单漂移。
//
// 同时按表 / constant 实际用到的 paradigm 裁剪输出 —— 全 base 表完全省略 _sep。
//
// 命名规则：JSON key 与 toml leaf 一致（snake_case 摊平），XML attr 名 = "sep_" + key。
// Go / Java 模板侧的 SepConfig 字段为 PascalCase / camelCase，对应同一 25 项。

use std::collections::BTreeSet;

use crate::model::{Constant, Table};
use crate::types::{Paradigm, SeparatorsSection, TblType};

/// 25 项分隔符 leaf 的真值源。顺序固定 —— JSON / XML 输出都按此顺序遍历。
pub fn sep_kv_pairs(sep: &SeparatorsSection) -> [(&'static str, &str); 25] {
    [
        ("list", &sep.list),
        ("set", &sep.set),
        ("tuple2", &sep.tuple2),
        ("tuple3", &sep.tuple3),
        ("tuple4", &sep.tuple4),
        ("map_kv", &sep.map.kv),
        ("map_entry", &sep.map.entry),
        ("list_tuple2_tuple", &sep.list_tuple2.tuple),
        ("list_tuple2_list", &sep.list_tuple2.list),
        ("list_tuple3_tuple", &sep.list_tuple3.tuple),
        ("list_tuple3_list", &sep.list_tuple3.list),
        ("list_tuple4_tuple", &sep.list_tuple4.tuple),
        ("list_tuple4_list", &sep.list_tuple4.list),
        ("map_tuple2_kv", &sep.map_tuple2.kv),
        ("map_tuple2_tuple", &sep.map_tuple2.tuple),
        ("map_tuple2_entry", &sep.map_tuple2.entry),
        ("map_tuple3_kv", &sep.map_tuple3.kv),
        ("map_tuple3_tuple", &sep.map_tuple3.tuple),
        ("map_tuple3_entry", &sep.map_tuple3.entry),
        ("map_tuple4_kv", &sep.map_tuple4.kv),
        ("map_tuple4_tuple", &sep.map_tuple4.tuple),
        ("map_tuple4_entry", &sep.map_tuple4.entry),
        ("map_list_kv", &sep.map_list.kv),
        ("map_list_item", &sep.map_list.item),
        ("map_list_entry", &sep.map_list.entry),
    ]
}

/// 单个 paradigm 用到的分隔符 key 列表。Base / Ref 不需要分隔符。
pub fn paradigm_sep_keys(p: &Paradigm) -> &'static [&'static str] {
    match p {
        Paradigm::Base | Paradigm::Ref => &[],
        Paradigm::Tuple2 => &["tuple2"],
        Paradigm::Tuple3 => &["tuple3"],
        Paradigm::Tuple4 => &["tuple4"],
        Paradigm::List => &["list"],
        Paradigm::Set => &["set"],
        Paradigm::Map => &["map_kv", "map_entry"],
        Paradigm::ListTuple2 => &["list_tuple2_tuple", "list_tuple2_list"],
        Paradigm::ListTuple3 => &["list_tuple3_tuple", "list_tuple3_list"],
        Paradigm::ListTuple4 => &["list_tuple4_tuple", "list_tuple4_list"],
        Paradigm::MapTuple2 => &["map_tuple2_kv", "map_tuple2_tuple", "map_tuple2_entry"],
        Paradigm::MapTuple3 => &["map_tuple3_kv", "map_tuple3_tuple", "map_tuple3_entry"],
        Paradigm::MapTuple4 => &["map_tuple4_kv", "map_tuple4_tuple", "map_tuple4_entry"],
        Paradigm::MapList => &["map_list_kv", "map_list_item", "map_list_entry"],
    }
}

fn collect_from_type(s: &str, used: &mut BTreeSet<&'static str>) {
    if let Some(t) = TblType::parse(s) {
        for k in paradigm_sep_keys(&t.paradigm) {
            used.insert(k);
        }
    }
}

/// 返回该 table 中**导出字段**用到的分隔符 key 集合。
pub fn collect_used_sep_keys_table(table: &Table) -> BTreeSet<&'static str> {
    use crate::model::Export;
    let mut used = BTreeSet::new();
    for f in &table.schema.fields {
        if matches!(f.export, Export::ClientServer | Export::ServerOnly) {
            collect_from_type(&f.tbl_type, &mut used);
        }
    }
    used
}

/// 返回该 constant 中**导出条目**用到的分隔符 key 集合。
pub fn collect_used_sep_keys_constant(constant: &Constant) -> BTreeSet<&'static str> {
    use crate::model::Export;
    let mut used = BTreeSet::new();
    for entry in &constant.entries {
        if matches!(entry.export, Export::ClientServer | Export::ServerOnly) {
            collect_from_type(&entry.tbl_type, &mut used);
        }
    }
    used
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_count_matches_separators_section() {
        // 25 项与 toml [separators] 配置一一对应。如果加新分隔符，这里和 SeparatorsSection 必须同步。
        let sep = SeparatorsSection::default();
        assert_eq!(sep_kv_pairs(&sep).len(), 25);
    }

    #[test]
    fn pairs_keys_unique() {
        let sep = SeparatorsSection::default();
        let pairs = sep_kv_pairs(&sep);
        let mut seen = BTreeSet::new();
        for (k, _) in pairs {
            assert!(seen.insert(k), "duplicate sep key: {}", k);
        }
    }

    #[test]
    fn paradigm_keys_subset_of_pairs() {
        let sep = SeparatorsSection::default();
        let all_keys: BTreeSet<&'static str> = sep_kv_pairs(&sep).into_iter().map(|(k, _)| k).collect();
        let paradigms = [
            Paradigm::Base, Paradigm::Ref,
            Paradigm::Tuple2, Paradigm::Tuple3, Paradigm::Tuple4,
            Paradigm::List, Paradigm::Set, Paradigm::Map,
            Paradigm::ListTuple2, Paradigm::ListTuple3, Paradigm::ListTuple4,
            Paradigm::MapTuple2, Paradigm::MapTuple3, Paradigm::MapTuple4,
            Paradigm::MapList,
        ];
        for p in &paradigms {
            for k in paradigm_sep_keys(p) {
                assert!(all_keys.contains(k), "{:?} references unknown key {}", p, k);
            }
        }
    }

    #[test]
    fn paradigm_keys_cover_all_pairs() {
        // 每个 leaf key 至少被一个 paradigm 用到，否则就是死配置。
        let sep = SeparatorsSection::default();
        let all_keys: BTreeSet<&'static str> = sep_kv_pairs(&sep).into_iter().map(|(k, _)| k).collect();
        let paradigms = [
            Paradigm::Tuple2, Paradigm::Tuple3, Paradigm::Tuple4,
            Paradigm::List, Paradigm::Set, Paradigm::Map,
            Paradigm::ListTuple2, Paradigm::ListTuple3, Paradigm::ListTuple4,
            Paradigm::MapTuple2, Paradigm::MapTuple3, Paradigm::MapTuple4,
            Paradigm::MapList,
        ];
        let mut covered = BTreeSet::new();
        for p in &paradigms {
            for k in paradigm_sep_keys(p) {
                covered.insert(*k);
            }
        }
        assert_eq!(covered, all_keys, "some sep keys are never used by any paradigm");
    }
}
