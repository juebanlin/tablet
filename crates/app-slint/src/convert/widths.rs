//! 列宽估算与合并（纯函数，无 Slint 状态）。
//!
//! Rust 侧没有文本测量 API；这里用字符宽度启发式（CJK/全角 1em，ASCII 0.56em），
//! 表头 11px、数据 12px，再留出箭头/内边距。估算不准时用户可以手动拖；自动宽度
//! 只在节点打开/重置时算一次，避免编辑时列跳动。

use std::collections::BTreeMap;

use slint::Model;

use super::grid::GridSnapshot;
use crate::state::ColumnKind;
use crate::CellKind;
#[cfg(test)]
use crate::{DataCell, HeaderCell};

pub const DEFAULT_COL_W: f32 = 100.0;
pub const MIN_AUTO_W: f32 = 60.0;
pub const MAX_AUTO_W: f32 = 480.0;
pub const MIN_MANUAL_W: f32 = 40.0;
pub const MAX_MANUAL_W: f32 = 800.0;
pub const SAMPLE_ROWS: usize = 200;

const HEADER_FONT_PX: f32 = 11.0;
const DATA_FONT_PX: f32 = 12.0;
const HEADER_PAD: f32 = 8.0;
const DATA_PAD: f32 = 4.0;
const ARROW_W: f32 = 16.0;

/// 单个字符占多少个 em（粗略字体度量）。
pub fn char_em(c: char) -> f32 {
    let cp = c as u32;
    let wide = (0x1100..=0x115F).contains(&cp)       // Hangul Jamo
        || (0x2E80..=0xA4CF).contains(&cp)           // CJK radicals / kana / ext-A
        || (0xAC00..=0xD7A3).contains(&cp)           // Hangul syllables
        || (0xF900..=0xFAFF).contains(&cp)           // CJK compatibility
        || (0xFE30..=0xFE4F).contains(&cp)           // CJK compatibility forms
        || (0xFF00..=0xFF60).contains(&cp)           // fullwidth forms
        || (0xFFE0..=0xFFE6).contains(&cp)
        || (0x1F300..=0x1FAFF).contains(&cp)         // emoji
        || (0x20000..=0x3FFFD).contains(&cp);        // CJK ext-B+
    if wide { 1.0 } else { 0.62 }
}

pub fn text_px(s: &str, font_px: f32) -> f32 {
    s.chars().map(|c| char_em(c) * font_px).sum()
}

/// 根据表头 / 数据样本估算每列宽度。
pub fn auto_widths(snap: &GridSnapshot) -> Vec<f32> {
    let cols = snap.col_count.max(0) as usize;
    if cols == 0 {
        return Vec::new();
    }
    let mut header = vec![0.0f32; cols];
    let mut header_arrow = vec![false; cols];
    for row in &snap.header_rows {
        for (ci, cell) in row.iter().enumerate().take(cols) {
            header[ci] = header[ci].max(text_px(cell.text.as_str(), HEADER_FONT_PX));
            if matches!(
                cell.kind,
                CellKind::TypeEnum
                    | CellKind::ExportEnum
                    | CellKind::TypeEnumCol
                    | CellKind::ExportEnumCol
            ) {
                header_arrow[ci] = true;
            }
        }
    }

    let mut data = vec![0.0f32; cols];
    let mut data_arrow = vec![false; cols];
    for (ci, kind) in snap.column_kinds.iter().enumerate().take(cols) {
        data_arrow[ci] = matches!(
            kind,
            ColumnKind::Ref { .. }
                | ColumnKind::RefList { .. }
                | ColumnKind::TypeEnumCol
                | ColumnKind::ExportEnumCol
        );
    }

    let rows = snap.data_rows.len();
    if rows > 0 {
        let stride = ((rows + SAMPLE_ROWS - 1) / SAMPLE_ROWS).max(1);
        let mut r = 0;
        while r < rows {
            sample_row(&snap.data_rows[r], cols, &mut data);
            r += stride;
        }
        // 始终看最后一行，避免抽样错过尾部超长值。
        if (rows - 1) % stride != 0 {
            sample_row(&snap.data_rows[rows - 1], cols, &mut data);
        }
    }

    (0..cols)
        .map(|i| {
            let h = header[i] + HEADER_PAD + if header_arrow[i] { ARROW_W } else { 0.0 };
            let d = data[i] + DATA_PAD + if data_arrow[i] { ARROW_W } else { DATA_PAD };
            // 字体度量是估算；留 8% 余量，避免 "WARRIOR" 这种刚好差几像素被省略。
            round_half((h.max(d) * 1.08).clamp(MIN_AUTO_W, MAX_AUTO_W))
        })
        .collect()
}

fn sample_row(row: &crate::DataRow, cols: usize, out: &mut [f32]) {
    let n = row.cells.row_count().min(cols);
    for ci in 0..n {
        if let Some(cell) = row.cells.row_data(ci) {
            out[ci] = out[ci].max(text_px(cell.text.as_str(), DATA_FONT_PX));
        }
    }
}

/// 合并自动/手动宽度：手动模式下列名命中则用手动值，否则回退到自动值。
pub fn merge_widths(
    names: &[String],
    auto: &[f32],
    manual: &BTreeMap<String, f32>,
    manual_mode: bool,
    col_count: usize,
) -> Vec<f32> {
    (0..col_count)
        .map(|i| {
            if manual_mode {
                if let Some(name) = names.get(i) {
                    if let Some(w) = manual.get(name).copied() {
                        if w.is_finite() {
                            return round_half(w.clamp(MIN_MANUAL_W, MAX_MANUAL_W));
                        }
                    }
                }
            }
            auto.get(i).copied().unwrap_or(DEFAULT_COL_W)
        })
        .collect()
}

pub fn total_width(widths: &[f32], col_count: usize) -> f32 {
    (0..col_count)
        .map(|i| widths.get(i).copied().unwrap_or(DEFAULT_COL_W))
        .sum()
}

/// x（内容坐标）落在哪一列；边界归右侧列，负 x → 0，超出 → 最后一列。
pub fn col_at_x(widths: &[f32], x: f32) -> usize {
    if widths.is_empty() {
        return 0;
    }
    if x <= 0.0 {
        return 0;
    }
    let mut acc = 0.0;
    for (i, w) in widths.iter().enumerate() {
        acc += *w;
        if x < acc {
            return i;
        }
    }
    widths.len() - 1
}

fn round_half(v: f32) -> f32 {
    (v * 2.0).round() / 2.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::convert::GridSnapshot;
    use crate::DataRow;
    use slint::{ModelRc, SharedString, VecModel};

    fn cell(text: &str) -> DataCell {
        DataCell {
            text: SharedString::from(text),
            has_error: false,
            is_empty_ref: false,
        }
    }

    fn row(texts: &[&str]) -> DataRow {
        DataRow {
            cells: ModelRc::new(VecModel::from(
                texts.iter().map(|t| cell(t)).collect::<Vec<_>>(),
            )),
            row_color: slint::Color::from_argb_u8(0, 0, 0, 0),
            selected: false,
        }
    }

    fn header(text: &str) -> HeaderCell {
        HeaderCell {
            text: SharedString::from(text),
            kind: CellKind::Text,
            color: slint::Color::from_argb_u8(0, 0, 0, 0),
            has_error: false,
        }
    }

    fn snap(headers: &[&str], rows: &[Vec<&str>], kinds: Vec<ColumnKind>) -> GridSnapshot {
        let mut s = GridSnapshot::empty();
        s.col_count = headers.len() as i32;
        s.column_names = headers.iter().map(|h| h.to_string()).collect();
        s.header_rows = vec![headers.iter().map(|h| header(h)).collect()];
        s.data_rows = rows.iter().map(|r| row(r)).collect();
        s.column_kinds = kinds;
        s
    }

    #[test]
    fn cjk_is_wider_than_ascii() {
        assert!(text_px("你好", 12.0) > text_px("ab", 12.0));
        assert_eq!(text_px("", 12.0), 0.0);
        assert!((text_px("a", 10.0) - 6.2).abs() < 0.01);
    }

    #[test]
    fn auto_width_uses_header_and_data_and_clamps() {
        let s = snap(
            &["id", "name", "desc"],
            &[
                vec!["1", "PLAYER_NOT_FOUND", "玩家不存在"],
                vec!["2", "SHORT", "短"],
            ],
            vec![ColumnKind::Text; 3],
        );
        let w = auto_widths(&s);
        assert_eq!(w.len(), 3);
        assert!(w.iter().all(|x| *x >= MIN_AUTO_W && *x <= MAX_AUTO_W));
        assert!(w[1] > w[0], "name 比 id 宽: {:?}", w);
        assert!(w[2] > w[0], "desc 比 id 宽: {:?}", w);

        let long = vec![vec![
            "1".to_string(),
            "X".repeat(500),
            "Y".repeat(500),
        ]];
        let refs: Vec<Vec<&str>> = long
            .iter()
            .map(|r| r.iter().map(|s| s.as_str()).collect())
            .collect();
        let s2 = snap(&["id", "name", "desc"], &refs, vec![ColumnKind::Text; 3]);
        let w2 = auto_widths(&s2);
        assert_eq!(w2[2], MAX_AUTO_W, "超长内容应被 clamp: {:?}", w2);
    }

    #[test]
    fn manual_widths_win_by_name_and_fallback_auto() {
        let names = vec!["id".to_string(), "name".to_string(), "desc".to_string()];
        let auto = vec![60.0, 120.0, 200.0];
        let mut manual = BTreeMap::new();
        manual.insert("name".to_string(), 300.0);

        let w = merge_widths(&names, &auto, &manual, true, 3);
        assert_eq!(w, vec![60.0, 300.0, 200.0]);

        let w2 = merge_widths(&names, &auto, &manual, false, 3);
        assert_eq!(w2, auto);

        // 列重排/新增后按列名对上；新列回退 auto
        let names2 = vec!["desc".to_string(), "name".to_string(), "new".to_string()];
        let auto2 = vec![210.0, 70.0, 90.0];
        let w3 = merge_widths(&names2, &auto2, &manual, true, 3);
        assert_eq!(w3, vec![210.0, 300.0, 90.0]);
    }

    #[test]
    fn manual_widths_are_sanitized() {
        let names = vec!["a".to_string()];
        let auto = vec![100.0];
        let mut manual = BTreeMap::new();
        manual.insert("a".to_string(), f32::NAN);
        assert_eq!(merge_widths(&names, &auto, &manual, true, 1), vec![100.0]);
        manual.insert("a".to_string(), 9000.0);
        assert_eq!(merge_widths(&names, &auto, &manual, true, 1), vec![MAX_MANUAL_W]);
    }

    #[test]
    fn col_at_x_boundaries() {
        let widths = [100.0, 50.0, 30.0];
        assert_eq!(col_at_x(&widths, -5.0), 0);
        assert_eq!(col_at_x(&widths, 0.0), 0);
        assert_eq!(col_at_x(&widths, 99.9), 0);
        assert_eq!(col_at_x(&widths, 100.0), 1);
        assert_eq!(col_at_x(&widths, 149.9), 1);
        assert_eq!(col_at_x(&widths, 150.0), 2);
        assert_eq!(col_at_x(&widths, 1e9), 2);
        assert_eq!(col_at_x(&[], 10.0), 0);
    }

    #[test]
    fn total_width_pads_and_sums() {
        assert_eq!(total_width(&[100.0, 50.0], 3), 250.0);
        assert_eq!(total_width(&[], 0), 0.0);
    }

    #[test]
    fn snapshot_empty_has_no_columns() {
        let s = GridSnapshot::empty();
        assert!(auto_widths(&s).is_empty());
        assert!(s.column_names.is_empty());
    }
}
