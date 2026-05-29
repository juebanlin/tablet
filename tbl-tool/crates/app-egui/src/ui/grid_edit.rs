use eframe::egui;
use super::grid_model::{CellKind, GridSource};
use super::grid::COL_W;
use crate::app::TblApp;
use tbl_core::model::Export;

pub fn render_edit(ui: &mut egui::Ui, app: &mut TblApp, kind: &CellKind, pos: egui::Pos2, col_w: f32, group: &str, name: &str, source: &GridSource) {
    match kind {
        CellKind::TypeEnum | CellKind::TypeEnumCol => {
            if !app.type_selector.open {
                if let Some(cell) = app.edit_state.editing.clone() {
                    app.type_selector.open_with(&app.edit_state.edit_buffer, cell, group, name, source);
                    app.edit_state.editing = None;
                }
            }
        }
        CellKind::Ref { name: ref_name } => {
            if !app.ref_picker.open {
                if let Some(cell) = app.edit_state.editing.clone() {
                    app.ref_picker.open_with(ref_name, &app.edit_state.edit_buffer, cell, group, name, source);
                    app.edit_state.editing = None;
                }
            }
        }
        CellKind::ExportEnum | CellKind::ExportEnumCol => {
            render_export_dropdown(ui, app, pos);
        }
        _ if kind.double_click_to_edit() => {
            render_text_input(ui, app, pos, col_w);
        }
        _ => {}
    }
}

fn render_export_dropdown(ui: &mut egui::Ui, app: &mut TblApp, pos: egui::Pos2) {
    egui::Area::new(egui::Id::new("grid_dropdown"))
        .fixed_pos(pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            ui.set_min_width(COL_W);
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                for opt in Export::options() {
                    let label = opt.display();
                    ui.set_min_width(COL_W - 8.0);
                    if ui.selectable_label(app.edit_state.edit_buffer == label, label).clicked() {
                        app.edit_state.edit_buffer = label.to_string();
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
