// 全 crate 共享的视觉常量：图标 / marker 字符 / 颜色。
//
// 任何 emoji 字符 / 颜色都应来自这里，而不是字面量散落在 convert / dialogs 里。
// `.slint` 文件里同样的字符是 preview mock 数据，运行时被这里 push 的字符串覆盖。

use slint::Color;

// ──────── 节点图标（树面板 + grid title） ────────
pub const ICON_PROJECT: &str = "📦";
pub const ICON_GROUP: &str = "📁";
pub const ICON_TABLE: &str = "📊";
pub const ICON_CONST: &str = "📋";
pub const ICON_ENUM: &str = "🔢";

// ──────── 装饰图标 ────────
/// active project 名字尾部的星号
pub const ACTIVE_STAR: &str = "⭐";
/// 状态栏 / 提示中的告警三角（无 variation selector）
pub const WARN: &str = "⚠";
/// 对话框标题里的告警（带 emoji variation selector，UI 字号下颜色更明显）
pub const WARN_EMOJI: &str = "⚠️";

// ──────── 引用类型标签（type_selector / ref_picker 共享） ────────
pub const REF_LABEL_TABLE: &str = "📊 表引用";
pub const REF_LABEL_ENUM: &str = "🔢 枚举引用";

// ──────── 节点 marker（树面板 deleted/new/modified/error 角标） ────────
pub const MARK_NEW: &str = "+";
pub const MARK_MOD: &str = "*";
pub const MARK_DEL: &str = "-";
pub const MARK_ERR: &str = "!";
pub const MARK_NONE: &str = "";

// ──────── 节点 marker 色 ────────
pub fn color_new() -> Color { Color::from_rgb_u8(40, 180, 40) }
pub fn color_mod() -> Color { Color::from_rgb_u8(200, 170, 0) }
pub fn color_del() -> Color { Color::from_rgb_u8(220, 50, 50) }
pub fn color_err() -> Color { Color::from_rgb_u8(220, 50, 50) }
pub fn color_default() -> Color { Color::from_rgb_u8(0xe6, 0xe6, 0xe6) }

// ──────── grid 文本色 ────────
pub fn color_text_primary() -> Color { Color::from_rgb_u8(0x1a, 0x1a, 0x1a) }
pub fn color_text_readonly() -> Color { Color::from_rgb_u8(0x6e, 0x6e, 0x6e) }
pub fn color_text_success() -> Color { Color::from_rgb_u8(0x50, 0xa0, 0x50) }
pub fn color_text_info() -> Color { Color::from_rgb_u8(0x50, 0x82, 0xd2) }
pub fn color_transparent() -> Color { Color::from_argb_u8(0, 0, 0, 0) }
