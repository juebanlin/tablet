use eframe::egui;
use super::grid_model::CellKind;
use super::grid::COL_W;
use crate::app::TblApp;

pub fn render_edit(ui: &mut egui::Ui, app: &mut TblApp, kind: &CellKind, pos: egui::Pos2, col_w: f32) {
    if kind.click_to_edit() {
        render_dropdown(ui, app, kind, pos);
    } else if kind.double_click_to_edit() {
        render_text_input(ui, app, pos, col_w);
    }
}

fn render_dropdown(ui: &mut egui::Ui, app: &mut TblApp, kind: &CellKind, pos: egui::Pos2) {
    let options = kind.enum_options();
    if options.is_empty() { return; }

    egui::Area::new(egui::Id::new("grid_dropdown"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            ui.set_min_width(COL_W);
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for &opt in options {
                    ui.set_min_width(COL_W - 8.0);
                    if ui.selectable_label(app.edit_state.edit_buffer == opt, opt).clicked() {
                        app.edit_state.edit_buffer = opt.to_string();
                        app.edit_state.commit_pending = true;
                    }
                }
            });
        });
}

fn render_text_input(ui: &mut egui::Ui, app: &mut TblApp, pos: egui::Pos2, col_w: f32) {
    egui::Area::new(egui::Id::new("grid_text_edit"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            let resp = ui.add(egui::TextEdit::singleline(&mut app.edit_state.edit_buffer)
                .desired_width(col_w - 4.0));
            resp.request_focus();
            if resp.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                app.edit_state.commit_pending = true;
            }
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.edit_state.editing = None;
            }
        });
}
