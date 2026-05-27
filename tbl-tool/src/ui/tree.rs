use eframe::egui;
use crate::app::{TblApp, SelectedNode, PendingAction, TreeFilter, TreeContext};

pub fn render(ui: &mut egui::Ui, app: &mut TblApp) {
    ui.heading("配置树");
    ui.horizontal(|ui| {
        let label = match app.tree_filter {
            TreeFilter::All => "全部",
            TreeFilter::New => "新增",
            TreeFilter::Modified => "修改",
            TreeFilter::Deleted => "删除",
            TreeFilter::Changed => "改动",
        };
        egui::ComboBox::from_id_source("tree_filter")
            .selected_text(label)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut app.tree_filter, TreeFilter::All, "全部");
                ui.selectable_value(&mut app.tree_filter, TreeFilter::Changed, "改动");
                ui.separator();
                ui.selectable_value(&mut app.tree_filter, TreeFilter::New, "新增");
                ui.selectable_value(&mut app.tree_filter, TreeFilter::Modified, "修改");
                ui.selectable_value(&mut app.tree_filter, TreeFilter::Deleted, "删除");
            });
        if app.tree_filter != TreeFilter::All {
            ui.checkbox(&mut app.tree_filter_show_full_group, "完整组");
        }
    });
    ui.separator();

    egui::ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| {
        let groups = app.project.groups.clone();
        let filter = app.tree_filter.clone();
        let show_full = app.tree_filter_show_full_group;

        for group in &groups {
            let group_has_match = filter == TreeFilter::All
                || group.tables.iter().any(|t| passes_filter(&filter, t.deleted, t.original.is_empty(), t.dirty))
                || group.constants.iter().any(|c| passes_filter(&filter, c.deleted, c.original.is_empty(), c.dirty));
            if !group_has_match { continue; }

            let expanded = app.tree_expanded.contains(&group.name);
            let arrow = if expanded { "▼" } else { "▶" };

            // Compute group status
            let all_deleted = !group.tables.is_empty() || !group.constants.is_empty();
            let all_deleted = all_deleted
                && group.tables.iter().all(|t| t.deleted)
                && group.constants.iter().all(|c| c.deleted);
            let has_dirty = group.tables.iter().any(|t| t.dirty && !t.deleted)
                || group.constants.iter().any(|c| c.dirty && !c.deleted);
            let group_deleted = all_deleted && !group.is_new;
            let group_is_new = group.is_new;
            let group_dirty = has_dirty && !group_is_new && !group_deleted;

            // Group row
            let row = ui.horizontal(|ui| {
                if ui.small_button(arrow).clicked() {
                    toggle_expanded(app, &group.name);
                }
                let group_label = format!("📁 {}", group.name);
                let r = ui.selectable_label(false, &group_label);
                render_marker(ui, group_deleted, group_is_new, group_dirty);
                r
            });
            let label_resp = row.inner;
            let row_rect = row.response.rect;

            if label_resp.double_clicked() {
                toggle_expanded(app, &group.name);
            }
            if check_secondary_click(ui, row_rect) {
                app.tree_context = Some(TreeContext::Group(group.name.clone()));
                app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
            }

            if expanded {
                for table in &group.tables {
                    if !show_full && !passes_filter(&filter, table.deleted, table.original.is_empty(), table.dirty) { continue; }
                    let selected = matches!(&app.selected, Some(SelectedNode::Table { group: g, name: n }) if g == &group.name && n == &table.name);
                    let row = ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        let r = ui.selectable_label(selected, format!("📊 {}", table.name));
                        render_marker(ui, table.deleted, table.original.is_empty(), table.dirty);
                        r
                    });
                    if row.inner.clicked() && !table.deleted {
                        app.selected = Some(SelectedNode::Table { group: group.name.clone(), name: table.name.clone() });
                    }
                    if check_secondary_click(ui, row.response.rect) {
                        app.tree_context = Some(TreeContext::Node { group: group.name.clone(), name: table.name.clone(), is_table: true });
                        app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
                    }
                }
                for constant in &group.constants {
                    if !show_full && !passes_filter(&filter, constant.deleted, constant.original.is_empty(), constant.dirty) { continue; }
                    let selected = matches!(&app.selected, Some(SelectedNode::Constant { group: g, name: n }) if g == &group.name && n == &constant.name);
                    let row = ui.horizontal(|ui| {
                        ui.add_space(18.0);
                        let r = ui.selectable_label(selected, format!("📋 {}", constant.name));
                        render_marker(ui, constant.deleted, constant.original.is_empty(), constant.dirty);
                        r
                    });
                    if row.inner.clicked() && !constant.deleted {
                        app.selected = Some(SelectedNode::Constant { group: group.name.clone(), name: constant.name.clone() });
                    }
                    if check_secondary_click(ui, row.response.rect) {
                        app.tree_context = Some(TreeContext::Node { group: group.name.clone(), name: constant.name.clone(), is_table: false });
                        app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
                    }
                }
            }
        }

        // Fill remaining vertical space
        let remaining = ui.available_rect_before_wrap();
        if remaining.height() > 0.0 {
            ui.allocate_rect(remaining, egui::Sense::hover());
        }
    });

    // Fallback: right-click anywhere in tree panel that wasn't caught by a specific item
    if app.tree_context.is_none() && ui.input(|i| i.pointer.secondary_clicked()) {
        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
            if ui.max_rect().contains(pos) {
                app.tree_context = Some(TreeContext::Blank);
                app.context_pos = pos;
            }
        }
    }

    render_tree_context(ui, app);
}

fn check_secondary_click(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.input(|i| {
        i.pointer.secondary_clicked()
            && i.pointer.interact_pos().map_or(false, |p| rect.contains(p))
    })
}

fn toggle_expanded(app: &mut TblApp, name: &str) {
    if app.tree_expanded.contains(name) {
        app.tree_expanded.remove(name);
    } else {
        app.tree_expanded.insert(name.to_string());
    }
}

fn render_tree_context(ui: &mut egui::Ui, app: &mut TblApp) {
    let ctx = match &app.tree_context { Some(c) => c.clone(), None => return };

    egui::Area::new(egui::Id::new("tree_ctx_menu"))
        .fixed_pos(app.context_pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(100.0);
                match &ctx {
                    TreeContext::Blank => {
                        if ui.button("新建 Group").clicked() {
                            app.pending_action = Some(PendingAction::NewGroup);
                            app.tree_context = None;
                        }
                    }
                    TreeContext::Group(name) => {
                        if ui.button("新建 Table").clicked() {
                            app.pending_action = Some(PendingAction::NewTable { group: name.clone() });
                            app.tree_context = None;
                        }
                        if ui.button("新建 Constant").clicked() {
                            app.pending_action = Some(PendingAction::NewConstant { group: name.clone() });
                            app.tree_context = None;
                        }
                        ui.separator();
                        if ui.button("重命名").clicked() {
                            app.pending_action = Some(PendingAction::RenameGroup { old_name: name.clone() });
                            app.tree_context = None;
                        }
                        if ui.button("删除").clicked() {
                            app.pending_action = Some(PendingAction::DeleteGroup { group: name.clone() });
                            app.tree_context = None;
                        }
                    }
                    TreeContext::Node { group, name, is_table } => {
                        if ui.button("复制").clicked() {
                            app.pending_action = Some(PendingAction::CopyNode { group: group.clone(), name: name.clone(), is_table: *is_table });
                            app.tree_context = None;
                        }
                        if ui.button("重命名").clicked() {
                            app.pending_action = Some(PendingAction::RenameNode { group: group.clone(), old_name: name.clone() });
                            app.tree_context = None;
                        }
                        ui.separator();
                        if ui.button("删除").clicked() {
                            app.pending_action = Some(PendingAction::DeleteNode { group: group.clone(), name: name.clone() });
                            app.tree_context = None;
                        }
                    }
                }
            });
        });

    if ui.input(|i| i.pointer.primary_clicked() || i.key_pressed(egui::Key::Escape)) {
        app.tree_context = None;
    }
}

fn passes_filter(filter: &TreeFilter, deleted: bool, is_new: bool, dirty: bool) -> bool {
    match filter {
        TreeFilter::All => true,
        TreeFilter::New => is_new && !deleted,
        TreeFilter::Modified => dirty && !is_new && !deleted,
        TreeFilter::Deleted => deleted,
        TreeFilter::Changed => deleted || dirty || is_new,
    }
}

fn render_marker(ui: &mut egui::Ui, deleted: bool, is_new: bool, dirty: bool) {
    let (text, color) = if deleted {
        ("-", egui::Color32::from_rgb(220, 50, 50))
    } else if is_new {
        ("+", egui::Color32::from_rgb(40, 180, 40))
    } else if dirty {
        ("*", egui::Color32::from_rgb(200, 170, 0))
    } else {
        return;
    };
    ui.label(egui::RichText::new(text).color(color).strong());
}