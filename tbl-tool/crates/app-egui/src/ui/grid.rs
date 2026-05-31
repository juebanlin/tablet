use eframe::egui;
use super::grid_model::*;
use super::grid_edit;
use crate::app::{TblApp, Selection, CellPos};

const ROW_H: f32 = 22.0;
pub const COL_W: f32 = 100.0;
const ROW_NUM_W: f32 = 32.0;
const BORDER: egui::Color32 = egui::Color32::from_gray(180);
const HEADER_BG: egui::Color32 = egui::Color32::from_gray(240);
const READONLY_HEADER_BG: egui::Color32 = egui::Color32::from_gray(200);
const READONLY_TEXT: egui::Color32 = egui::Color32::from_gray(110);
const SEL_BG: egui::Color32 = egui::Color32::from_rgba_premultiplied(180, 215, 255, 255);
const SEL_BORDER: egui::Color32 = egui::Color32::from_rgb(50, 120, 200);
const ERROR_BORDER: egui::Color32 = egui::Color32::from_rgb(220, 50, 50);
const EXTRA_ROWS: usize = 5;

pub fn render_grid(ui: &mut egui::Ui, app: &mut TblApp, group: &str, name: &str, grid: &GridData) {
    ui.spacing_mut().item_spacing.y = 0.0;
    let col_count = grid.col_defs.len();
    let display_rows = grid.data_count + EXTRA_ROWS;
    let total_w = ROW_NUM_W + COL_W * col_count as f32;
    let header_h = ROW_H * (1 + grid.header_rows.len()) as f32; // +1 for col letter row
    let data_h = ROW_H * display_rows as f32;

    // --- Fixed header ---
    let (h_resp, h_painter) = ui.allocate_painter(egui::vec2(total_w, header_h), egui::Sense::click());
    let ho = h_resp.rect.min;
    h_painter.rect_filled(egui::Rect::from_min_size(ho, egui::vec2(total_w, header_h)), 0.0, HEADER_BG);

    // Col letter row
    for col in 0..col_count {
        let x = ho.x + ROW_NUM_W + col as f32 * COL_W;
        draw_center(&h_painter, x, ho.y, COL_W, &col_letter(col), 11.0, egui::Color32::GRAY);
    }
    // Col selection highlight on letter row
    if let Selection::Col(c) = app.edit_state.selected {
        if c < col_count {
            let r = egui::Rect::from_min_size(egui::pos2(ho.x + ROW_NUM_W + c as f32 * COL_W, ho.y), egui::vec2(COL_W, ROW_H));
            h_painter.rect_filled(r, 0.0, SEL_BG);
        }
    }

    // Header content rows
    for (hrow, cells) in grid.header_rows.iter().enumerate() {
        let y = ho.y + (1 + hrow) as f32 * ROW_H;
        for (col, cell) in cells.iter().enumerate() {
            let x = ho.x + ROW_NUM_W + col as f32 * COL_W;
            let cell_rect = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(COL_W, ROW_H));
            let is_readonly = matches!(cell.kind, CellKind::ReadOnly);
            if is_readonly {
                h_painter.rect_filled(cell_rect, 0.0, READONLY_HEADER_BG);
            }
            if matches!(app.edit_state.selected, Selection::Col(c) if c == col) {
                h_painter.rect_filled(cell_rect, 0.0, SEL_BG);
            }
            let text_color = if is_readonly { READONLY_TEXT } else { cell.color };
            draw_center(&h_painter, x, y, COL_W, &display_for_header(grid, hrow, &cell.kind, &cell.text), 11.0, text_color);
            if cell.kind.show_dropdown_arrow() {
                draw_dropdown_arrow(&h_painter, x + COL_W - 14.0, y + ROW_H / 2.0);
            }
        }
    }

    // Header grid lines
    for row in 0..=(1 + grid.header_rows.len()) {
        let y = ho.y + row as f32 * ROW_H;
        h_painter.line_segment([egui::pos2(ho.x, y), egui::pos2(ho.x + total_w, y)], egui::Stroke::new(0.5, BORDER));
    }
    h_painter.line_segment([egui::pos2(ho.x, ho.y), egui::pos2(ho.x, ho.y + header_h)], egui::Stroke::new(0.5, BORDER));
    for col in 0..=col_count {
        let x = ho.x + ROW_NUM_W + col as f32 * COL_W;
        h_painter.line_segment([egui::pos2(x, ho.y), egui::pos2(x, ho.y + header_h)], egui::Stroke::new(0.5, BORDER));
    }

    // Header interaction
    handle_header_click(app, &h_resp, ho, grid, group, name, col_count);

    // --- Scrollable data area ---
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let (resp, painter) = ui.allocate_painter(egui::vec2(total_w, data_h), egui::Sense::click_and_drag());
        let o = resp.rect.min;

        // Row number bg
        painter.rect_filled(egui::Rect::from_min_size(o, egui::vec2(ROW_NUM_W, data_h)), 0.0, HEADER_BG);

        // Draw rows
        for row in 0..display_rows {
            let y = o.y + row as f32 * ROW_H;
            let row_selected = matches!(app.edit_state.selected, Selection::Row(r) if r == row)
                || matches!(app.edit_state.selected, Selection::Rows(s, e) if row >= s && row <= e);
            if row_selected {
                painter.rect_filled(egui::Rect::from_min_size(egui::pos2(o.x, y), egui::vec2(ROW_NUM_W, ROW_H)), 0.0, SEL_BG);
            }
            draw_center(&painter, o.x, y, ROW_NUM_W, &format!("{}", row + 1), 11.0, egui::Color32::GRAY);

            for col in 0..col_count {
                let x = o.x + ROW_NUM_W + col as f32 * COL_W;
                let r = egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(COL_W, ROW_H));

                if is_selected(&app.edit_state.selected, row, col) {
                    painter.rect_filled(r, 0.0, SEL_BG);
                    if app.edit_state.selected == Selection::Cell(row, col) {
                        painter.rect_stroke(r, 0.0, egui::Stroke::new(2.0, SEL_BORDER));
                    }
                }

                if app.engine.validation_errors.contains(&(group.to_string(), name.to_string(), row, col)) {
                    painter.rect_stroke(r, 0.0, egui::Stroke::new(1.5, ERROR_BORDER));
                }

                if row < grid.data.len() {
                    let val = grid.data[row].get(col).cloned().unwrap_or_default();
                    let display = display_for_cell(app, grid, col, &val);
                    painter.text(egui::pos2(x + 3.0, y + ROW_H / 2.0), egui::Align2::LEFT_CENTER,
                        &display, egui::FontId::proportional(12.0), egui::Color32::BLACK);
                }

                let is_valid_row = row < grid.data_count;
                if grid.col_defs[col].kind.show_dropdown_arrow() && is_valid_row {
                    draw_dropdown_arrow(&painter, x + COL_W - 14.0, y + ROW_H / 2.0);
                }
            }
        }

        // Grid lines
        for row in 0..=display_rows {
            let y = o.y + row as f32 * ROW_H;
            painter.line_segment([egui::pos2(o.x, y), egui::pos2(o.x + total_w, y)], egui::Stroke::new(0.5, BORDER));
        }
        painter.line_segment([egui::pos2(o.x, o.y), egui::pos2(o.x, o.y + data_h)], egui::Stroke::new(0.5, BORDER));
        for col in 0..=col_count {
            let x = o.x + ROW_NUM_W + col as f32 * COL_W;
            painter.line_segment([egui::pos2(x, o.y), egui::pos2(x, o.y + data_h)], egui::Stroke::new(0.5, BORDER));
        }

        // Hover tracking
        app.edit_state.hover_cell = resp.hover_pos().and_then(|pos| {
            let rel = pos - o;
            if rel.x > ROW_NUM_W {
                let col = ((rel.x - ROW_NUM_W) / COL_W) as usize;
                let row = (rel.y / ROW_H) as usize;
                if col < col_count && row < display_rows { Some((row, col)) } else { None }
            } else { None }
        });

        // Data interaction
        handle_data_interaction(app, &resp, o, grid, group, name, col_count, display_rows);
    });

    // Inline edit overlay
    if let Some(ref editing) = app.edit_state.editing.clone() {
        if let Some(pos) = app.edit_state.edit_pos {
            let kind = resolve_edit_kind(editing, grid);
            grid_edit::render_edit(ui, app, &kind, pos, COL_W, group, name, &grid.source);
        }
    }

    // Handle commit
    if app.edit_state.commit_pending {
        app.edit_state.commit_pending = false;
        if let Some(editing) = app.edit_state.editing.clone() {
            app.grid_commit(group, name, &editing, &grid.source);
        }
    }

    // Keyboard shortcuts
    handle_keys(ui, app, group, name, grid);
}

fn resolve_edit_kind(editing: &CellPos, grid: &GridData) -> CellKind {
    if editing.header_row.is_some() {
        let hrow = editing.header_row.unwrap();
        if hrow < grid.header_rows.len() {
            if let Some(cell) = grid.header_rows[hrow].get(editing.col) {
                return cell.kind.clone();
            }
        }
        return CellKind::Text;
    }
    if editing.col < grid.col_defs.len() {
        return grid.col_defs[editing.col].kind.clone();
    }
    CellKind::Text
}

fn handle_header_click(app: &mut TblApp, resp: &egui::Response, ho: egui::Pos2, grid: &GridData, group: &str, name: &str, col_count: usize) {
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let rel = pos - ho;
            commit_current_edit(app, group, name, grid);
            app.context_col = None;
            app.context_row = None;

            if rel.y < ROW_H && rel.x > ROW_NUM_W {
                let col = ((rel.x - ROW_NUM_W) / COL_W) as usize;
                if col < col_count { app.edit_state.selected = Selection::Col(col); }
            }
        }
    }
    if resp.double_clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let rel = pos - ho;
            if rel.y >= ROW_H && rel.x > ROW_NUM_W {
                let hrow = ((rel.y - ROW_H) / ROW_H) as usize;
                let col = ((rel.x - ROW_NUM_W) / COL_W) as usize;
                if hrow < grid.header_rows.len() && col < col_count {
                    let kind = &grid.header_rows[hrow][col].kind;
                    if kind.double_click_to_edit() {
                        let cell_x = ho.x + ROW_NUM_W + col as f32 * COL_W;
                        // picker 类弹窗从 cell 下方展开，文本编辑则盖在 cell 上
                        let cell_y = ho.y + (1 + hrow) as f32 * ROW_H
                            + if kind.is_picker() { ROW_H } else { 0.0 };
                        app.edit_state.editing = Some(CellPos { row: usize::MAX, col, header_row: Some(hrow) });
                        app.edit_state.edit_buffer = grid.header_rows[hrow][col].text.clone();
                        app.edit_state.edit_pos = Some(egui::pos2(cell_x, cell_y));
                    }
                }
            }
        }
    }
    if resp.secondary_clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            app.context_pos = pos;
            let rel = pos - ho;
            if rel.y < ROW_H && rel.x > ROW_NUM_W {
                let col = ((rel.x - ROW_NUM_W) / COL_W) as usize;
                if col < col_count { app.context_col = Some(col); }
            }
        }
    }
}

fn handle_data_interaction(app: &mut TblApp, resp: &egui::Response, o: egui::Pos2, grid: &GridData, group: &str, name: &str, col_count: usize, display_rows: usize) {
    if resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let rel = pos - o;
            commit_current_edit(app, group, name, grid);
            app.context_col = None;
            app.context_row = None;

            if rel.x <= ROW_NUM_W {
                let row = (rel.y / ROW_H) as usize;
                if row < display_rows { app.edit_state.selected = Selection::Row(row); }
            } else {
                let col = ((rel.x - ROW_NUM_W) / COL_W) as usize;
                let row = (rel.y / ROW_H) as usize;
                if col < col_count && row < display_rows {
                    app.edit_state.selected = Selection::Cell(row, col);
                } else {
                    app.edit_state.selected = Selection::None;
                }
            }
        }
    }

    if resp.double_clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let rel = pos - o;
            if rel.x > ROW_NUM_W {
                let col = ((rel.x - ROW_NUM_W) / COL_W) as usize;
                let row = (rel.y / ROW_H) as usize;
                if col < col_count && row < display_rows {
                    let kind = &grid.col_defs[col].kind;
                    let is_valid_row = row < grid.data_count;
                    let allow = kind.double_click_to_edit()
                        && (!kind.is_picker() || is_valid_row);
                    if allow {
                        let val = grid.data.get(row).and_then(|r| r.get(col)).cloned().unwrap_or_default();
                        let cell_x = o.x + ROW_NUM_W + col as f32 * COL_W;
                        // picker 类弹窗从 cell 下方展开，文本编辑则盖在 cell 上
                        let cell_y = o.y + row as f32 * ROW_H
                            + if kind.is_picker() { ROW_H } else { 0.0 };
                        app.edit_state.editing = Some(CellPos { row, col, header_row: None });
                        app.edit_state.edit_buffer = val;
                        app.edit_state.edit_pos = Some(egui::pos2(cell_x, cell_y));
                        app.edit_state.selected = Selection::Cell(row, col);
                    }
                }
            }
        }
    }

    if resp.secondary_clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            app.context_pos = pos;
            let rel = pos - o;
            if rel.x <= ROW_NUM_W {
                let row = (rel.y / ROW_H) as usize;
                if row < grid.data_count { app.context_row = Some(row); }
            }
        }
    }

    // Drag selection
    if resp.drag_started() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let rel = pos - o;
            if rel.x > ROW_NUM_W {
                let col = ((rel.x - ROW_NUM_W) / COL_W) as usize;
                let row = (rel.y / ROW_H) as usize;
                if col < col_count && row < display_rows {
                    app.edit_state.drag_start = Some((row, col));
                    app.edit_state.selected = Selection::Cell(row, col);
                }
            } else {
                let row = (rel.y / ROW_H) as usize;
                if row < display_rows {
                    app.edit_state.drag_start = None;
                    app.edit_state.selected = Selection::Row(row);
                }
            }
        }
    }
    if resp.dragged() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let rel = pos - o;
            if rel.x <= ROW_NUM_W {
                let row = ((rel.y / ROW_H) as usize).min(display_rows - 1);
                match app.edit_state.selected {
                    Selection::Row(start) | Selection::Rows(start, _) => {
                        let (s, e) = if row >= start { (start, row) } else { (row, start) };
                        app.edit_state.selected = Selection::Rows(s, e);
                    }
                    _ => {}
                }
            } else {
                let col = (((rel.x - ROW_NUM_W) / COL_W) as usize).min(col_count - 1);
                let row = ((rel.y / ROW_H) as usize).min(display_rows - 1);
                let start = app.edit_state.drag_start.unwrap_or_else(|| {
                    app.edit_state.drag_start = Some((row, col));
                    (row, col)
                });
                if start == (row, col) {
                    app.edit_state.selected = Selection::Cell(row, col);
                } else {
                    app.edit_state.selected = Selection::CellRange { start, end: (row, col) };
                }
            }
        }
    }
    if resp.drag_stopped() {
        app.edit_state.drag_start = None;
    }
}

fn handle_keys(ui: &mut egui::Ui, app: &mut TblApp, group: &str, name: &str, grid: &GridData) {
    // 任何 editing（文本 inline 或 picker 弹窗）都把 grid 快捷键全屏蔽，
    // 避免 picker/编辑器内部按键被 grid 抢去触发复制/粘贴/删除。
    let is_editing = app.edit_state.editing.is_some();

    // Ctrl+C
    if !is_editing {
        let ctrl_c = ui.input(|i| {
            i.events.iter().any(|e| matches!(e, egui::Event::Copy)) || (i.modifiers.ctrl && i.key_pressed(egui::Key::C))
        });
        if ctrl_c {
            let text = copy_selected_text(app, grid);
            if !text.is_empty() {
                if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
                app.log("[快捷键] 已复制".to_string());
            }
        }
    }

    if is_editing { return; }

    // Ctrl+V
    let ctrl_v = ui.input(|i| {
        i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))) || (i.modifiers.ctrl && i.key_pressed(egui::Key::V))
    });
    if ctrl_v {
        if let Ok(mut cb) = arboard::Clipboard::new() {
            if let Ok(text) = cb.get_text() {
                let (sr, sc) = match app.edit_state.selected {
                    Selection::Cell(r, c) => (r, c),
                    Selection::CellRange { start, .. } => start,
                    Selection::Row(r) => (r, 0),
                    _ => (grid.data_count, 0),
                };
                app.paste_data(group, name, sr, sc, &text, &grid.source);
                app.log("[快捷键] 已粘贴".to_string());
            }
        }
    }

    // Delete
    if ui.input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace)) {
        app.delete_selected(group, name, grid);
    }
}

fn commit_current_edit(app: &mut TblApp, group: &str, name: &str, grid: &GridData) {
    if !app.auto_commit_on_blur {
        app.edit_state.editing = None;
        return;
    }
    if let Some(editing) = app.edit_state.editing.clone() {
        app.grid_commit(group, name, &editing, &grid.source);
    }
}

pub fn copy_selected_text(app: &TblApp, grid: &GridData) -> String {
    let cell = |r: usize, c: usize| -> String {
        grid.data.get(r).and_then(|row| row.get(c)).cloned().unwrap_or_default()
    };
    match &app.edit_state.selected {
        Selection::Cell(r, c) => cell(*r, *c),
        Selection::CellRange { start, end } => {
            let (r0, r1) = (start.0.min(end.0), start.0.max(end.0));
            let (c0, c1) = (start.1.min(end.1), start.1.max(end.1));
            (r0..=r1).map(|r| {
                (c0..=c1).map(|c| cell(r, c)).collect::<Vec<_>>().join("\t")
            }).collect::<Vec<_>>().join("\n")
        }
        Selection::Row(r) => (0..grid.col_defs.len()).map(|c| cell(*r, c)).collect::<Vec<_>>().join("\t"),
        Selection::Rows(s, e) => (*s..=*e).map(|r|
            (0..grid.col_defs.len()).map(|c| cell(r, c)).collect::<Vec<_>>().join("\t")
        ).collect::<Vec<_>>().join("\n"),
        Selection::Col(c) => (0..grid.data.len()).map(|r| cell(r, *c)).collect::<Vec<_>>().join("\n"),
        Selection::None => String::new(),
    }
}

/// 把存储值转换为单元格展示值。规则参见 grid_model.rs 顶部说明：
/// - Export 列：cs/c/s/- → 前后端/客户端/服务器/不导出
/// - @EnumName 引用列 + 功能区开关：id → entry.name（未匹配/表引用/空值原样）
/// - 其它：原样
pub(crate) fn display_for_cell(app: &TblApp, grid: &GridData, col: usize, raw: &str) -> String {
    use crate::ui::grid_model::CellKind;
    if raw.is_empty() { return String::new(); }
    let kind = grid.col_defs.get(col).map(|c| &c.kind);
    match kind {
        Some(CellKind::ExportEnum) | Some(CellKind::ExportEnumCol) => {
            tbl_core::model::Export::from_str(raw).display().to_string()
        }
        Some(CellKind::Ref { name }) if app.view_show_enum_name => {
            for g in &app.engine.project.groups {
                for e in &g.enums {
                    if e.deleted || e.name != *name { continue; }
                    if let Some(entry) = e.entries.iter().find(|en| en.id == raw && !en.name.is_empty()) {
                        return entry.name.clone();
                    }
                    return raw.to_string();
                }
            }
            raw.to_string()
        }
        _ => raw.to_string(),
    }
}

/// 把存储值转换为表头单元格展示值（不依赖 app/groups）：
/// - Table 的 export 行（hrow=1）一律 cs/c/s/- → 前后端/客户端/服务器/不导出，
///   即使 id 列 kind=ReadOnly 也走转换。展示与可编辑性是独立维度。
/// - 其它：原样
fn display_for_header(grid: &GridData, hrow: usize, kind: &crate::ui::grid_model::CellKind, raw: &str) -> String {
    use crate::ui::grid_model::CellKind;
    if raw.is_empty() { return String::new(); }
    let is_export_row = matches!(grid.source, GridSource::Table) && hrow == 1;
    if is_export_row || matches!(kind, CellKind::ExportEnum | CellKind::ExportEnumCol) {
        return tbl_core::model::Export::from_str(raw).display().to_string();
    }
    raw.to_string()
}

fn is_selected(sel: &Selection, row: usize, col: usize) -> bool {
    match sel {
        Selection::Cell(r, c) => *r == row && *c == col,
        Selection::CellRange { start, end } => {
            let (r0, r1) = (start.0.min(end.0), start.0.max(end.0));
            let (c0, c1) = (start.1.min(end.1), start.1.max(end.1));
            row >= r0 && row <= r1 && col >= c0 && col <= c1
        }
        Selection::Row(r) => *r == row,
        Selection::Rows(s, e) => row >= *s && row <= *e,
        Selection::Col(c) => *c == col,
        Selection::None => false,
    }
}

pub fn col_letter(idx: usize) -> String {
    let mut r = String::new();
    let mut n = idx;
    loop { r.insert(0, (b'A' + (n % 26) as u8) as char); if n < 26 { break; } n = n / 26 - 1; }
    r
}

pub fn format_selection(sel: &Selection, hover: Option<(usize, usize)>) -> String {
    let mut s = match sel {
        Selection::None => String::new(),
        Selection::Cell(r, c) => format!("{}{}", col_letter(*c), r + 1),
        Selection::CellRange { start, end } => {
            let (r0, r1) = (start.0.min(end.0), start.0.max(end.0));
            let (c0, c1) = (start.1.min(end.1), start.1.max(end.1));
            let rows = r1 - r0 + 1;
            let cols = c1 - c0 + 1;
            format!("{}{}:{}{} ({}行×{}列，共{}格)",
                col_letter(c0), r0 + 1, col_letter(c1), r1 + 1, rows, cols, rows * cols)
        }
        Selection::Row(r) => format!("第{}行", r + 1),
        Selection::Rows(s, e) => format!("第{}-{}行 (共{}行)", s + 1, e + 1, e - s + 1),
        Selection::Col(c) => format!("第{}列", col_letter(*c)),
    };
    if let Some((r, c)) = hover {
        if !s.is_empty() { s.push_str("  "); }
        s.push_str(&format!("光标: {}{}", col_letter(c), r + 1));
    }
    s
}

fn draw_center(painter: &egui::Painter, x: f32, y: f32, w: f32, text: &str, size: f32, color: egui::Color32) {
    painter.text(egui::pos2(x + w / 2.0, y + ROW_H / 2.0), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(size), color);
}

fn draw_dropdown_arrow(painter: &egui::Painter, x: f32, cy: f32) {
    let s = 4.0;
    painter.add(egui::Shape::convex_polygon(
        vec![egui::pos2(x - s, cy - s * 0.5), egui::pos2(x + s, cy - s * 0.5), egui::pos2(x, cy + s * 0.5)],
        egui::Color32::from_gray(120), egui::Stroke::NONE,
    ));
}
