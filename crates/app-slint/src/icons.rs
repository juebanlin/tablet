// UI 字形的唯一真理源：emoji / 符号字面量全部集中在这里。
//
// 结构分两层（关键）：
//   1. `G_*` 原始字形 —— 每个字面量在整个 crate **只写一次**，只在本文件出现。
//   2. 语义常量 —— 允许**多个语义指向同一个原始字形**（如展开三角与单元格下拉箭头
//      都是 ▼）。想换字形只改第 1 层一处，所有别名自动跟着变，不会漏改。
//
// 为什么要有这个文件（@docs/04-UI设计.md §10）：
//   - 跨平台字体覆盖参差。实测 U+23CE(⏎)、U+25BE/U+25B8(▾▸) 在 Windows 默认字体链里
//     就是豆腐块，而 U+25BC/U+25B6(▼▶)、U+2026(…) 正常。这类"哪个能画"的结论只有
//     集中放才查得动、换得动。
//   - slint 是纯展示层，rust 与 .slint 都要用同一批字形。.slint 不能 import rust 常量，
//     所以约定：**字面量只写在本文件**，启动时由 `wire()` 一次性推给 `ui/icons.slint`
//     的 `global Icons`，.slint 侧只引用 `Icons.xxx`，不写字面量。
//
// 新增字形三步：① 这里加 `G_*` 原始字形 + `pub const` 语义别名
//             ② ui/icons.slint 加同名 kebab-case 属性
//             ③ wire() 里推一次
// 漏 ② 编译不过（属性不存在）；漏 ③ 则 .slint 侧拿到空串，图标不显示。

use slint::ComponentHandle as _;

use crate::AppWindow;

// ════════════════ ① 原始字形（字面量只在下面出现一次）════════════════

const G_PROJECT: &str = "📦";
const G_GROUP: &str = "📁";
/// Xlsx 组：数据在一个工作簿里（节点 = sheet），与目录型的 📁 区分开。
const G_GROUP_XLSX: &str = "📗";
const G_TABLE: &str = "📊";
const G_CONST: &str = "📋";
const G_ENUM: &str = "🔢";
const G_STAR: &str = "⭐";
const G_WARN: &str = "⚠";
const G_WARN_EMOJI: &str = "⚠️"; // 带 variation selector U+FE0F
const G_LOCK: &str = "🔒";
const G_FILE: &str = "📄";
const G_SEARCH: &str = "🔍";
const G_BLOCK: &str = "⛔";
const G_OK: &str = "✅";
// 展开三角。别换成同族的小三角变体 ▾(U+25BE) / ▸(U+25B8) —— 那两个是豆腐块。
const G_TRI_DOWN: &str = "▼";
const G_TRI_RIGHT: &str = "▶";
const G_CHECK_ON: &str = "☑";
const G_CHECK_OFF: &str = "☐";
const G_CHECK_PARTIAL: &str = "▣";
const G_ARROW_LEFT: &str = "←";
const G_ARROW_RIGHT: &str = "→";
const G_NEW: &str = "🆕";
const G_MOD: &str = "📝";
const G_DEL: &str = "🗑️";
const G_ERR: &str = "🚨";
// 还原到已保存。
//
// 别再用 ↺(U+21BA)：本机 Windows 字体链里**实测是豆腐块**（已加进下面测试的 BANNED）。
// 顺带一个教训 —— 它和能画的 ←(U+2190) / →(U+2192) 同在 Arrows 块（U+2190–U+21FF），
// "同块必能画"这个推断不成立；只有 emoji 那一族是稳定可画的。
const G_REVERT: &str = "🔄";

// ════════════════ ② 语义常量（多个可指向同一原始字形）════════════════

// ──────── 节点图标（树面板 + grid 标题） ────────
pub const ICON_PROJECT: &str = G_PROJECT;
pub const ICON_GROUP: &str = G_GROUP;
pub const ICON_GROUP_XLSX: &str = G_GROUP_XLSX;
pub const ICON_TABLE: &str = G_TABLE;
pub const ICON_CONST: &str = G_CONST;
pub const ICON_ENUM: &str = G_ENUM;

// ──────── 展开 / 下拉三角 ────────
/// 树节点、公式栏的「已展开」三角
pub const TRI_EXPANDED: &str = G_TRI_DOWN;
/// 单元格下拉箭头 —— 与 TRI_EXPANDED 是同一个 ▼
pub const DROPDOWN_ARROW: &str = G_TRI_DOWN;
/// 树节点、公式栏的「已收起」三角
pub const TRI_COLLAPSED: &str = G_TRI_RIGHT;

// ──────── 状态 / 装饰 ────────
/// active project 名字尾部的星号
pub const ACTIVE_STAR: &str = G_STAR;
/// 状态栏 / 提示中的告警三角（无 variation selector）
pub const WARN: &str = G_WARN;
/// 对话框 / 提示里的告警（带 variation selector，UI 字号下颜色更明显）
pub const WARN_EMOJI: &str = G_WARN_EMOJI;
/// 公式栏 / readonly 单元格右侧的锁标记
pub const ICON_READONLY_LOCK: &str = G_LOCK;
/// Excel 文件卡片 / 文件浏览器
pub const ICON_FILE: &str = G_FILE;
/// 搜索框放大镜
pub const ICON_SEARCH: &str = G_SEARCH;
/// 禁止 / 阻塞（Excel 同步面板不可用行的标记）
pub const ICON_BLOCK: &str = G_BLOCK;
/// 成功 / 新增
pub const ICON_OK: &str = G_OK;

/// 「还原到已保存」：项目设置每个字段右侧的还原按钮、grid 区功能 bar 的 [还原]（@02.26）
pub const ICON_REVERT: &str = G_REVERT;

// ──────── 复选框三态（schema 导入勾选） ────────
pub const CHECK_ON: &str = G_CHECK_ON;
pub const CHECK_OFF: &str = G_CHECK_OFF;
pub const CHECK_PARTIAL: &str = G_CHECK_PARTIAL;

// ──────── 方向箭头（Excel 同步面板的数据流向标注） ────────
pub const ARROW_LEFT: &str = G_ARROW_LEFT;
pub const ARROW_RIGHT: &str = G_ARROW_RIGHT;

// ──────── 节点 marker（树面板 deleted/new/modified/error 角标） ────────
//
// 使用动作语义 emoji，颜色由字形自带；mark-color 文字色仍保留供未来纯文本 fallback。
pub const MARK_NEW: &str = G_NEW;
pub const MARK_MOD: &str = G_MOD;
pub const MARK_DEL: &str = G_DEL;
pub const MARK_ERR: &str = G_ERR;
pub const MARK_NONE: &str = "";

// ════════════════ ③ 复合标签（字形 + 文字）════════════════
//
// 用函数而不是 const：字形来自上面的 G_*，不在这里重抄一遍字面量
// （const 拼字符串要 concat!，而 concat! 只吃字面量、吃不了 const 标识符）。

/// 引用选择器标题：`📊 表引用` / `🔢 枚举引用`
pub fn ref_label_table() -> String { format!("{} 表引用", G_TABLE) }
pub fn ref_label_enum() -> String { format!("{} 枚举引用", G_ENUM) }

/// 类型选择器一行的短标签：`📊 表` / `🔢 枚举`
pub fn pick_label_table() -> String { format!("{} 表", G_TABLE) }
pub fn pick_label_enum() -> String { format!("{} 枚举", G_ENUM) }

// ════════════════ 推给 slint ════════════════

/// 把所有字形推给 `ui/icons.slint` 的 `global Icons`。启动时调一次（main.rs）。
pub fn wire(ui: &AppWindow) {
    let i = ui.global::<crate::Icons>();
    i.set_tri_expanded(TRI_EXPANDED.into());
    i.set_tri_collapsed(TRI_COLLAPSED.into());
    i.set_dropdown(DROPDOWN_ARROW.into());
    i.set_lock(ICON_READONLY_LOCK.into());
    i.set_check_on(CHECK_ON.into());
    i.set_check_off(CHECK_OFF.into());
    i.set_check_partial(CHECK_PARTIAL.into());
    i.set_ok(ICON_OK.into());
    i.set_warn(WARN_EMOJI.into());
    i.set_block(ICON_BLOCK.into());
    i.set_ref_label_table(ref_label_table().into());
    i.set_ref_label_enum(ref_label_enum().into());
    i.set_pick_table(pick_label_table().into());
    i.set_pick_enum(pick_label_enum().into());
    i.set_arrow_left(ARROW_LEFT.into());
    i.set_arrow_right(ARROW_RIGHT.into());
    i.set_file(ICON_FILE.into());
    i.set_search(ICON_SEARCH.into());
    i.set_revert(ICON_REVERT.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 原始字形表：新增字形时这里也要加一行，测试守住两条不变式。
    const PRIMITIVES: &[(&str, &str)] = &[
        ("G_PROJECT", G_PROJECT), ("G_GROUP", G_GROUP), ("G_GROUP_XLSX", G_GROUP_XLSX), ("G_TABLE", G_TABLE),
        ("G_CONST", G_CONST), ("G_ENUM", G_ENUM), ("G_STAR", G_STAR),
        ("G_WARN", G_WARN), ("G_WARN_EMOJI", G_WARN_EMOJI), ("G_LOCK", G_LOCK),
        ("G_FILE", G_FILE), ("G_SEARCH", G_SEARCH), ("G_BLOCK", G_BLOCK),
        ("G_OK", G_OK), ("G_TRI_DOWN", G_TRI_DOWN), ("G_TRI_RIGHT", G_TRI_RIGHT),
        ("G_CHECK_ON", G_CHECK_ON), ("G_CHECK_OFF", G_CHECK_OFF),
        ("G_CHECK_PARTIAL", G_CHECK_PARTIAL), ("G_ARROW_LEFT", G_ARROW_LEFT),
        ("G_ARROW_RIGHT", G_ARROW_RIGHT), ("G_NEW", G_NEW), ("G_MOD", G_MOD),
        ("G_DEL", G_DEL), ("G_ERR", G_ERR), ("G_REVERT", G_REVERT),
    ];

    /// 别名成立：语义常量确实指向原始字形，而不是各写各的字面量。
    /// 换字形时只要第 1 层改了，这里就会跟着变 —— 校验的是"没被绕开"。
    #[test]
    fn semantic_constants_alias_the_primitives() {
        assert_eq!(ICON_TABLE, G_TABLE);
        assert_eq!(ICON_ENUM, G_ENUM);
        // 展开三角与下拉箭头共用同一个 ▼（这正是"允许多个常量共用一个字形"的场景）
        assert_eq!(TRI_EXPANDED, DROPDOWN_ARROW);
        assert_eq!(TRI_EXPANDED, G_TRI_DOWN);
        assert_eq!(TRI_COLLAPSED, G_TRI_RIGHT);
        assert_ne!(TRI_EXPANDED, TRI_COLLAPSED);
    }

    /// 复合标签的字形前缀来自 G_*，不是另抄一份。
    #[test]
    fn composite_labels_use_the_shared_glyph() {
        assert!(ref_label_table().starts_with(G_TABLE));
        assert!(pick_label_table().starts_with(G_TABLE));
        assert!(ref_label_enum().starts_with(G_ENUM));
        assert!(pick_label_enum().starts_with(G_ENUM));
    }

    /// 跨平台坑位守门：这几个字形实测过、能画；同族变体是豆腐块，别混进来。
    #[test]
    fn glyphs_stay_within_the_measured_safe_set() {
        // 实测渲染成方块，出现过就不许再进来。
        // ↺(U+21BA) 是 2026-10 在「还原到已保存」那个按钮上试出来的。
        const BANNED: &[char] = &[
            '\u{23CE}', '\u{25BE}', '\u{25B8}', '\u{25B9}', '\u{25BF}', '\u{21BA}',
        ];
        for (name, g) in PRIMITIVES {
            for ch in g.chars() {
                assert!(!BANNED.contains(&ch), "{name} 含实测会渲染成方块的字形 {ch:?} (U+{:04X})", ch as u32);
            }
        }
    }

    /// 守 wire() 与 icons.slint 的接缝：.slint 声明了哪个属性，wire() 就必须推哪个。
    ///
    /// 漏推不会编译失败（属性存在，只是值是空串），表现为"图标不显示"，很容易漏。
    /// 这里直接读源码比对，比跑起来看更早发现。
    #[test]
    fn every_declared_icon_property_is_wired() {
        let slint_src = include_str!("../ui/icons.slint");
        let rust_src = include_str!("icons.rs");
        let mut checked = 0;
        for line in slint_src.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("in-out property <string>") else { continue };
            let name = rest.trim().trim_end_matches(';').trim();
            if name.is_empty() { continue; }
            let setter = format!("set_{}(", name.replace('-', "_"));
            assert!(
                rust_src.contains(&setter),
                "ui/icons.slint 声明了 `{name}`，但 icons.rs::wire() 里没有 `{setter}` —— 运行时该图标会是空串"
            );
            checked += 1;
        }
        assert!(checked >= 15, "只解析到 {checked} 个属性，icons.slint 的格式是不是变了？");
    }

    /// 原始字形之间不该重复 —— 重复说明该合并成一个常量 + 多个别名。
    #[test]
    fn primitives_are_distinct() {
        for (i, (na, a)) in PRIMITIVES.iter().enumerate() {
            for (nb, b) in &PRIMITIVES[i + 1..] {
                assert_ne!(a, b, "{na} 与 {nb} 是同一个字形，应合并成一个原始常量");
            }
        }
    }
}
