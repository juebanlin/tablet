//! `[ui]` 段的可编辑叶子：一个 key 空间，显示 / 写入 / 比对 / 还原四件事共用。
//!
//! 与 [`ExportKey`](crate::export_keys::ExportKey) 同一套路子，只是 `UiConfig` 是扁的、
//! 只有一个 `Option`（`log_level`），所以既没有祖先链要裁剪，也不需要「形状还原」。
//!
//! 全局设置对话框是「编辑缓冲 + 打开时快照」的范式（@02.26.6）：**参照物是打开对话框
//! 那一刻的 `tablet.toml` 值**，而不是像项目设置那样住在数据结构上的 `original_*`。
//! 因为全局设置的「确定」就是写盘点，「内存 ≠ 盘上」这段窗口恰好只有对话框开着的时候，
//! 关掉就没有东西需要留存了。

use crate::config_leaf::{bool_str, set_enum, set_plain_bool, set_plain_enum};
use crate::model::UiConfig;

/// `[ui]` 段里一个可编辑叶子。`as_str()` 就是 UI 与测试共同认的那个 key。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiKey {
    AutoCommitOnBlur,
    RealtimeValidate,
    LogLevel,
    RefPickerStrategy,
    PickerTriggerHeader,
    PickerTriggerData,
    ShowMetaId,
    ConstantRefAllowed,
}

impl UiKey {
    /// 全部 8 项。顺序 = UI 上从上到下的顺序。
    pub const ALL: [UiKey; 8] = [
        UiKey::AutoCommitOnBlur,
        UiKey::RealtimeValidate,
        UiKey::LogLevel,
        UiKey::RefPickerStrategy,
        UiKey::PickerTriggerHeader,
        UiKey::PickerTriggerData,
        UiKey::ShowMetaId,
        UiKey::ConstantRefAllowed,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutoCommitOnBlur => "auto_commit_on_blur",
            Self::RealtimeValidate => "realtime_validate",
            Self::LogLevel => "log_level",
            Self::RefPickerStrategy => "ref_picker_strategy",
            Self::PickerTriggerHeader => "picker_trigger_header",
            Self::PickerTriggerData => "picker_trigger_data",
            Self::ShowMetaId => "show_meta_id",
            Self::ConstantRefAllowed => "constant_ref_allowed",
        }
    }

    /// 反查。未知 key = None —— **调用方必须把 None 当错误处理**，别 `_ => {}` 吞掉。
    pub fn from_str(s: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|k| k.as_str() == s)
    }

    /// 显示值：与 UI 上看到的那一格完全一致（含 `log_level` 的 `unwrap_or(默认)`）。
    /// 只用于渲染，不用于比对 —— 比对见 [`modified`](Self::modified)。
    pub fn get(self, cfg: &UiConfig) -> String {
        match self {
            Self::AutoCommitOnBlur => bool_str(cfg.auto_commit_on_blur),
            Self::RealtimeValidate => bool_str(cfg.realtime_validate),
            Self::LogLevel => cfg.log_level.unwrap_or_default().as_str().to_string(),
            Self::RefPickerStrategy => cfg.ref_picker.default_strategy.as_str().to_string(),
            Self::PickerTriggerHeader => cfg.picker_trigger_header.as_str().to_string(),
            Self::PickerTriggerData => cfg.picker_trigger_data.as_str().to_string(),
            Self::ShowMetaId => bool_str(cfg.show_meta_id),
            Self::ConstantRefAllowed => bool_str(cfg.constant_ref_allowed),
        }
    }

    /// 这个叶子和 `orig` 一样吗。
    ///
    /// **先比「设没设」，再比值** —— 只比 `get()` 会把 `None` 与 `Some(默认值)` 判成相等，
    /// 于是「改过但看不出改了什么」就成立了。这里只有 `log_level` 是 `Option`，
    /// 但规则统一，将来给 `UiConfig` 加 `Option` 字段时不用回来改。
    pub fn modified(self, live: &UiConfig, orig: &UiConfig) -> bool {
        if self.is_set(live) != self.is_set(orig) {
            return true;
        }
        self.get(live) != self.get(orig)
    }

    fn is_set(self, cfg: &UiConfig) -> bool {
        match self {
            Self::LogLevel => cfg.log_level.is_some(),
            // 其余都是裸字段，「设没设」恒为真
            _ => true,
        }
    }

    /// 写入人给的值。返回是否接受 —— 非法值保持原样并返回 false，由调用方去吵。
    pub fn set(self, cfg: &mut UiConfig, v: &str) -> bool {
        use crate::enums::{LogLevel, PickerTrigger, RefPickerStrategy};

        match self {
            Self::AutoCommitOnBlur => set_plain_bool(&mut cfg.auto_commit_on_blur, v),
            Self::RealtimeValidate => set_plain_bool(&mut cfg.realtime_validate, v),
            Self::LogLevel => set_enum::<LogLevel>(&mut cfg.log_level, v),
            Self::RefPickerStrategy => set_plain_enum::<RefPickerStrategy>(
                &mut cfg.ref_picker.default_strategy, v),
            Self::PickerTriggerHeader => set_plain_enum::<PickerTrigger>(
                &mut cfg.picker_trigger_header, v),
            Self::PickerTriggerData => set_plain_enum::<PickerTrigger>(
                &mut cfg.picker_trigger_data, v),
            Self::ShowMetaId => set_plain_bool(&mut cfg.show_meta_id, v),
            Self::ConstantRefAllowed => set_plain_bool(&mut cfg.constant_ref_allowed, v),
        }
    }

    /// 把这一个叶子还原成 `orig` 的样子。没改过就什么都不做。
    pub fn revert(self, live: &mut UiConfig, orig: &UiConfig) {
        if !self.modified(live, orig) {
            return;
        }
        match self {
            Self::AutoCommitOnBlur => live.auto_commit_on_blur = orig.auto_commit_on_blur,
            Self::RealtimeValidate => live.realtime_validate = orig.realtime_validate,
            Self::LogLevel => live.log_level = orig.log_level,
            Self::RefPickerStrategy => {
                live.ref_picker.default_strategy = orig.ref_picker.default_strategy
            }
            Self::PickerTriggerHeader => live.picker_trigger_header = orig.picker_trigger_header,
            Self::PickerTriggerData => live.picker_trigger_data = orig.picker_trigger_data,
            Self::ShowMetaId => live.show_meta_id = orig.show_meta_id,
            Self::ConstantRefAllowed => live.constant_ref_allowed = orig.constant_ref_allowed,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    /// 每个 key 一个和默认值不同的合法值。
    fn sentinel(k: UiKey) -> &'static str {
        use crate::enums::{LogLevel, PickerTrigger, RefPickerStrategy};

        fn pick(all: &[&'static str], cur: &str) -> &'static str {
            *all.iter().find(|s| **s != cur).expect("枚举至少要有两个选项")
        }

        let d = UiConfig::default();
        match k {
            UiKey::LogLevel => pick(&LogLevel::all_str(), d.log_level.unwrap_or_default().as_str()),
            UiKey::RefPickerStrategy => {
                pick(&RefPickerStrategy::all_str(), d.ref_picker.default_strategy.as_str())
            }
            UiKey::PickerTriggerHeader => {
                pick(&PickerTrigger::all_str(), d.picker_trigger_header.as_str())
            }
            UiKey::PickerTriggerData => {
                pick(&PickerTrigger::all_str(), d.picker_trigger_data.as_str())
            }
            // 布尔叶子：读当前值再取反。别写死 "false" —— `realtime_validate` 与
            // `show_meta_id` 默认就是 false，写 "false" 等于没改。
            _ => if k.get(&d) == "true" { "false" } else { "true" },
        }
    }

    #[test]
    fn all_count_matches_the_settings_form() {
        assert_eq!(UiKey::ALL.len(), 8);
    }

    #[test]
    fn keys_are_unique_and_round_trip() {
        let mut seen = BTreeSet::new();
        for k in UiKey::ALL {
            assert!(seen.insert(k.as_str()), "重复的 key: {}", k.as_str());
            assert_eq!(UiKey::from_str(k.as_str()), Some(k), "{} 反查不回来", k.as_str());
        }
        assert_eq!(UiKey::from_str("bogus"), None);
    }

    /// 每个 key 必须真的落到自己那一格：写一个哨兵值，只有它自己算改过。
    #[test]
    fn each_key_writes_only_its_own_leaf() {
        for k in UiKey::ALL {
            let orig = UiConfig::default();
            let mut live = orig.clone();
            assert!(!k.modified(&live, &orig), "{} 起点就该是干净的", k.as_str());

            let v = sentinel(k);
            assert!(k.set(&mut live, v), "{} 拒绝了哨兵值 {:?}", k.as_str(), v);
            assert_eq!(k.get(&live), v, "{} 写进去又读不回来", k.as_str());
            assert!(k.modified(&live, &orig), "{} 改了却不算脏", k.as_str());

            for other in UiKey::ALL {
                if other != k {
                    assert!(!other.modified(&live, &orig),
                        "改 {} 却连带弄脏了 {}", k.as_str(), other.as_str());
                }
            }
        }
    }

    #[test]
    fn revert_restores_every_leaf_and_clears_modified() {
        for k in UiKey::ALL {
            let orig = UiConfig::default();
            let mut live = orig.clone();
            assert!(k.set(&mut live, sentinel(k)));
            k.revert(&mut live, &orig);
            assert!(!k.modified(&live, &orig), "{} 还原后仍报改过", k.as_str());
        }
    }

    /// `log_level` 是唯一一个 `Option`：从 `Some(x)` 还原成 `None` 要真的回到 `None`，
    /// 不能写成 `Some(默认值)`（那会让 `tablet.toml` 无端多出一个键）。
    #[test]
    fn revert_puts_an_unset_option_back_to_unset() {
        let orig = UiConfig { log_level: None, ..UiConfig::default() };
        let mut live = orig.clone();
        assert!(UiKey::LogLevel.set(&mut live, "warn"));
        assert!(UiKey::LogLevel.modified(&live, &orig));

        UiKey::LogLevel.revert(&mut live, &orig);
        assert_eq!(live.log_level, None, "该回到 None，不是 Some(默认值)");
        assert!(!UiKey::LogLevel.modified(&live, &orig));
    }

    #[test]
    fn set_rejects_a_value_it_cannot_parse() {
        let mut cfg = UiConfig::default();
        assert!(!UiKey::LogLevel.set(&mut cfg, "chatty"), "非法日志级别不该被接受");
        assert_eq!(UiKey::LogLevel.get(&cfg), UiConfig::default().log_level.unwrap().as_str(),
            "拒绝之后要保持原样，不是清空");
        assert!(!UiKey::AutoCommitOnBlur.set(&mut cfg, "yes"), "布尔只认 true/false");
        assert!(cfg.auto_commit_on_blur, "拒绝之后要保持原样");
    }
}
