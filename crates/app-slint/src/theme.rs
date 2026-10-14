// 全 crate 共享的**颜色**常量。
//
// 字形（emoji / 符号）不在这里 —— 那些归 `src/icons.rs`（@docs/04-UI设计.md §10）。
// 本文件只管颜色，`theme.slint` 管尺寸 / 间距 / 字号。
//
// 早期版本把图标也放在这里，并在 `.slint` 里写重复的字面量当 preview mock；
// 那导致"同一个图标散在 rust + 多个 .slint"里，跨平台排查（哪个字形会渲染成方块）
// 得全局 grep。现已收拢到 icons.rs + ui/icons.slint 的 global Icons。

use slint::Color;

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
