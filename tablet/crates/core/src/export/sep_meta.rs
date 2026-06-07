// 分隔符元数据：按 paradigm 收集 `_sep` / `sep_*` 输出时实际用到的 SepKey 集合。
//
// JSON / XML 头部输出时直接遍历 SepKey::ALL，按本模块返回的 used 集合裁剪。
// 字面量一律走 types::SepKey；新增 / 删除 leaf 在 types.rs 改 enum，本模块不动。

use std::collections::BTreeSet;

use crate::model::{Constant, Table};
use crate::types::{SepKey, TblType};

fn collect_from_type(s: &str, used: &mut BTreeSet<SepKey>) {
    if let Some(t) = TblType::parse(s) {
        for k in t.paradigm.sep_keys() {
            used.insert(*k);
        }
    }
}

/// 返回该 table 中**导出字段**用到的分隔符集合。
pub fn collect_used_sep_keys_table(table: &Table) -> BTreeSet<SepKey> {
    use crate::model::Export;
    let mut used = BTreeSet::new();
    for f in &table.schema.fields {
        if matches!(f.export, Export::ClientServer | Export::ServerOnly) {
            collect_from_type(&f.tbl_type, &mut used);
        }
    }
    used
}

/// 返回该 constant 中**导出条目**用到的分隔符集合。
pub fn collect_used_sep_keys_constant(constant: &Constant) -> BTreeSet<SepKey> {
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
    use crate::types::Paradigm;

    #[test]
    fn sepkey_all_count() {
        assert_eq!(SepKey::ALL.len(), 25);
    }

    #[test]
    fn export_keys_unique() {
        let mut seen = BTreeSet::new();
        for k in SepKey::ALL {
            assert!(seen.insert(k.as_export_key()), "duplicate export key: {}", k.as_export_key());
        }
    }

    #[test]
    fn directive_keys_unique() {
        let mut seen = BTreeSet::new();
        for k in SepKey::ALL {
            assert!(seen.insert(k.as_directive_key()), "duplicate directive key: {}", k.as_directive_key());
        }
    }

    #[test]
    fn paradigm_keys_cover_all_sepkeys() {
        // 每个 SepKey 至少被一个 paradigm 用到，否则就是死配置。
        let paradigms = [
            Paradigm::Tuple2, Paradigm::Tuple3, Paradigm::Tuple4,
            Paradigm::List, Paradigm::Set, Paradigm::Map,
            Paradigm::ListTuple2, Paradigm::ListTuple3, Paradigm::ListTuple4,
            Paradigm::MapTuple2, Paradigm::MapTuple3, Paradigm::MapTuple4,
            Paradigm::MapList,
        ];
        let mut covered: BTreeSet<SepKey> = BTreeSet::new();
        for p in &paradigms {
            for k in p.sep_keys() {
                covered.insert(*k);
            }
        }
        let all: BTreeSet<SepKey> = SepKey::ALL.iter().copied().collect();
        assert_eq!(covered, all, "some sep keys are never used by any paradigm");
    }

    #[test]
    fn directive_key_roundtrip() {
        for k in SepKey::ALL {
            assert_eq!(SepKey::from_directive_key(k.as_directive_key()), Some(k));
        }
        assert_eq!(SepKey::from_directive_key("Bogus.key"), None);
    }
}
