use eframe::egui;
use crate::app::{TblApp, SelectedNode, PendingAction};

pub fn render(ui: &mut egui::Ui, app: &mut TblApp) {
    ui.heading("配置树");
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        let groups = app.project.groups.clone();
        for group in &groups {
            let id = ui.make_persistent_id(&group.name);
            egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true)
                .show_header(ui, |ui| {
                    let resp = ui.label(format!("📁 {}", group.name));
                    resp.context_menu(|ui| {
                        if ui.button("新建 Table").clicked() {
                            app.pending_action = Some(PendingAction::NewTable {
                                group: group.name.clone(),
                            });
                            ui.close_menu();
                        }
                        if ui.button("新建 Constant").clicked() {
                            app.pending_action = Some(PendingAction::NewConstant {
                                group: group.name.clone(),
                            });
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button("重命名").clicked() {
                            app.pending_action = Some(PendingAction::RenameGroup {
                                old_name: group.name.clone(),
                            });
                            ui.close_menu();
                        }
                        if ui.button("删除").clicked() {
                            app.pending_action = Some(PendingAction::DeleteGroup {
                                group: group.name.clone(),
                            });
                            ui.close_menu();
                        }
                    });
                })
                .body(|ui| {
                    for table in &group.tables {
                        let selected = matches!(&app.selected, Some(SelectedNode::Table { group: g, name: n }) if g == &group.name && n == &table.name);
                        let label = if table.dirty { format!("  📊 {} *", table.name) } else { format!("  📊 {}", table.name) };
                        let resp = ui.selectable_label(selected, label);
                        if resp.clicked() {
                            app.selected = Some(SelectedNode::Table {
                                group: group.name.clone(),
                                name: table.name.clone(),
                            });
                        }
                        resp.context_menu(|ui| {
                            if ui.button("复制").clicked() {
                                app.pending_action = Some(PendingAction::CopyNode {
                                    group: group.name.clone(),
                                    name: table.name.clone(),
                                    is_table: true,
                                });
                                ui.close_menu();
                            }
                            if ui.button("重命名").clicked() {
                                app.pending_action = Some(PendingAction::RenameNode {
                                    group: group.name.clone(),
                                    old_name: table.name.clone(),
                                });
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("删除").clicked() {
                                app.pending_action = Some(PendingAction::DeleteNode {
                                    group: group.name.clone(),
                                    name: table.name.clone(),
                                });
                                ui.close_menu();
                            }
                        });
                    }
                    for constant in &group.constants {
                        let selected = matches!(&app.selected, Some(SelectedNode::Constant { group: g, name: n }) if g == &group.name && n == &constant.name);
                        let label = if constant.dirty { format!("  📋 {} *", constant.name) } else { format!("  📋 {}", constant.name) };
                        let resp = ui.selectable_label(selected, label);
                        if resp.clicked() {
                            app.selected = Some(SelectedNode::Constant {
                                group: group.name.clone(),
                                name: constant.name.clone(),
                            });
                        }
                        resp.context_menu(|ui| {
                            if ui.button("复制").clicked() {
                                app.pending_action = Some(PendingAction::CopyNode {
                                    group: group.name.clone(),
                                    name: constant.name.clone(),
                                    is_table: false,
                                });
                                ui.close_menu();
                            }
                            if ui.button("重命名").clicked() {
                                app.pending_action = Some(PendingAction::RenameNode {
                                    group: group.name.clone(),
                                    old_name: constant.name.clone(),
                                });
                                ui.close_menu();
                            }
                            ui.separator();
                            if ui.button("删除").clicked() {
                                app.pending_action = Some(PendingAction::DeleteNode {
                                    group: group.name.clone(),
                                    name: constant.name.clone(),
                                });
                                ui.close_menu();
                            }
                        });
                    }
                });
        }

        let remaining = ui.available_rect_before_wrap();
        let resp = ui.allocate_rect(remaining, egui::Sense::click());
        resp.context_menu(|ui| {
            if ui.button("新建 Group").clicked() {
                app.pending_action = Some(PendingAction::NewGroup);
                ui.close_menu();
            }
        });
    });
}
