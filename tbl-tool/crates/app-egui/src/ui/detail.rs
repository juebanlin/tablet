use eframe::egui;
use crate::app::{TblApp, SelectedNode, Selection};
use super::grid_model::*;
use super::grid;

pub fn render(ui: &mut egui::Ui, app: &mut TblApp) {
    let bg_resp = ui.interact(ui.available_rect_before_wrap(), egui::Id::new("detail_bg"), egui::Sense::click());
    if bg_resp.clicked() {
        if app.edit_state.editing.is_some() {
            if app.auto_commit_on_blur {
                app.edit_state.commit_pending = true;
            } else {
                app.edit_state.editing = None;
            }
        }
        app.edit_state.selected = Selection::None;
        app.context_col = None;
        app.context_row = None;
    }

    let selected = app.selected.clone();
    match &selected {
        None => { ui.centered_and_justified(|ui| { ui.label("选择左侧节点查看详情"); }); }
        Some(SelectedNode::Table { group, name }) => {
            let grid_data = build_table_grid(app, group, name);
            if let Some(gd) = grid_data {
                let heading_resp = ui.heading(format!("📊 {} ({}条)", name, gd.data_count));
                if heading_resp.clicked() { app.edit_state.selected = Selection::None; app.edit_state.editing = None; }
                render_formula_bar(ui, app, group, name, &gd);
                grid::render_grid(ui, app, group, name, &gd);
            }
        }
        Some(SelectedNode::Constant { group, name }) => {
            let grid_data = build_constant_grid(app, group, name);
            if let Some(gd) = grid_data {
                let heading_resp = ui.heading(format!("📋 {} ({}项)", name, gd.data_count));
                if heading_resp.clicked() { app.edit_state.selected = Selection::None; app.edit_state.editing = None; }
                render_formula_bar(ui, app, group, name, &gd);
                grid::render_grid(ui, app, group, name, &gd);
            }
        }
        Some(SelectedNode::Enum { group, name }) => {
            let grid_data = build_enum_grid(app, group, name);
            if let Some(gd) = grid_data {
                let heading_resp = ui.heading(format!("🔢 {} ({}项)", name, gd.data_count));
                if heading_resp.clicked() { app.edit_state.selected = Selection::None; app.edit_state.editing = None; }
                render_formula_bar(ui, app, group, name, &gd);
                grid::render_grid(ui, app, group, name, &gd);
            }
        }
    }

    // Right-click context menus
    render_col_context(ui, app);
    render_row_context(ui, app);
    render_cell_context(ui, app);
}

fn render_formula_bar(ui: &mut egui::Ui, app: &mut TblApp, group: &str, name: &str, grid: &GridData) {
    let (label, editable, cell_val) = match &app.edit_state.selected {
        Selection::Cell(r, c) => {
            let coord = format!("{}{}", grid::col_letter(*c), r + 1);
            let kind = if *c < grid.col_defs.len() && *r < grid.data_count {
                &grid.col_defs[*c].kind
            } else {
                &CellKind::Text
            };
            let val = grid.data.get(*r).and_then(|row| row.get(*c)).cloned().unwrap_or_default();
            (coord, kind.double_click_to_edit(), val)
        }
        _ => (String::new(), false, String::new()),
    };

    let formula_id = egui::Id::new("formula_bar_input");
    let has_focus = ui.ctx().memory(|m| m.has_focus(formula_id));
    if !has_focus && editable {
        app.edit_state.formula_buffer = cell_val.clone();
    }

    ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), 22.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_width(40.0);
        ui.label(egui::RichText::new(&label).size(11.0).monospace());
        ui.separator();
        if editable {
            let resp = ui.add(egui::TextEdit::singleline(&mut app.edit_state.formula_buffer)
                .id(formula_id)
                .desired_width(ui.available_width()));
            if resp.lost_focus() {
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                if enter || app.auto_commit_on_blur {
                    app.edit_state.formula_committed = true;
                }
            }
        } else {
            ui.label(egui::RichText::new(&cell_val).size(11.0));
        }
    });

    if app.edit_state.formula_committed {
        app.edit_state.formula_committed = false;
        if let Selection::Cell(row, col) = app.edit_state.selected {
            let val = app.edit_state.formula_buffer.clone();
            app.set_cell_value(group, name, row, col, &val, &grid.source);
        }
    }
}

fn build_table_grid(app: &TblApp, group: &str, name: &str) -> Option<GridData> {
    let table = app.find_table(group, name)?;
    let fields = &table.schema.fields;

    let desc_row: Vec<HeaderCell> = fields.iter().map(|f| HeaderCell {
        text: f.desc.clone(), kind: CellKind::Text, color: egui::Color32::BLACK,
    }).collect();
    let export_row: Vec<HeaderCell> = fields.iter().map(|f| HeaderCell {
        text: f.export.display().to_string(), kind: CellKind::ExportEnum, color: egui::Color32::from_rgb(80, 160, 80),
    }).collect();
    let type_row: Vec<HeaderCell> = fields.iter().map(|f| HeaderCell {
        text: f.tbl_type.clone(), kind: CellKind::TypeEnum, color: egui::Color32::from_rgb(80, 130, 210),
    }).collect();
    let field_row: Vec<HeaderCell> = fields.iter().map(|f| {
        let kind = if f.name == "id" { CellKind::ReadOnly } else { CellKind::Text };
        HeaderCell { text: f.name.clone(), kind, color: egui::Color32::BLACK }
    }).collect();

    let col_defs: Vec<ColDef> = fields.iter().map(|f| {
        let kind = if let Some(name) = f.tbl_type.strip_prefix('@') {
            CellKind::Ref { name: name.trim().to_string() }
        } else {
            CellKind::Text
        };
        ColDef { kind }
    }).collect();

    let valid_count = table.records.iter()
        .filter(|r| r.first().map_or(false, |id| !id.is_empty()))
        .count();

    Some(GridData {
        source: GridSource::Table,
        header_rows: vec![desc_row, export_row, type_row, field_row],
        col_defs,
        data: table.records.clone(),
        data_count: valid_count,
    })
}

fn build_constant_grid(app: &TblApp, group: &str, name: &str) -> Option<GridData> {
    let constant = app.find_constant(group, name)?;

    let header_row = vec![
        HeaderCell { text: "name".to_string(), kind: CellKind::ReadOnly, color: egui::Color32::BLACK },
        HeaderCell { text: "type".to_string(), kind: CellKind::ReadOnly, color: egui::Color32::from_rgb(80, 130, 210) },
        HeaderCell { text: "value".to_string(), kind: CellKind::ReadOnly, color: egui::Color32::BLACK },
        HeaderCell { text: "export".to_string(), kind: CellKind::ReadOnly, color: egui::Color32::from_rgb(80, 160, 80) },
        HeaderCell { text: "desc".to_string(), kind: CellKind::ReadOnly, color: egui::Color32::BLACK },
    ];

    let col_defs = vec![
        ColDef { kind: CellKind::Text },
        ColDef { kind: CellKind::TypeEnumCol },
        ColDef { kind: CellKind::Text },
        ColDef { kind: CellKind::ExportEnumCol },
        ColDef { kind: CellKind::Text },
    ];

    let data: Vec<Vec<String>> = constant.entries.iter().map(|e| {
        vec![e.name.clone(), e.tbl_type.clone(), e.value.clone(), e.export.display().to_string(), e.desc.clone()]
    }).collect();

    let valid_count = data.iter().filter(|r| !r[0].is_empty()).count();

    Some(GridData {
        source: GridSource::Constant,
        header_rows: vec![header_row],
        col_defs,
        data_count: valid_count,
        data,
    })
}

fn build_enum_grid(app: &TblApp, group: &str, name: &str) -> Option<GridData> {
    let enum_def = app.find_enum(group, name)?;

    let header_row = vec![
        HeaderCell { text: "id".to_string(), kind: CellKind::ReadOnly, color: egui::Color32::BLACK },
        HeaderCell { text: "name".to_string(), kind: CellKind::ReadOnly, color: egui::Color32::BLACK },
        HeaderCell { text: "desc".to_string(), kind: CellKind::ReadOnly, color: egui::Color32::BLACK },
    ];

    let col_defs = vec![
        ColDef { kind: CellKind::Text },
        ColDef { kind: CellKind::Text },
        ColDef { kind: CellKind::Text },
    ];

    let data: Vec<Vec<String>> = enum_def.entries.iter()
        .map(|e| vec![e.id.clone(), e.name.clone(), e.desc.clone()])
        .collect();

    let valid_count = data.iter().filter(|r| !r[0].is_empty() || !r[1].is_empty()).count();

    Some(GridData {
        source: GridSource::Enum,
        header_rows: vec![header_row],
        col_defs,
        data_count: valid_count,
        data,
    })
}

fn render_col_context(ui: &mut egui::Ui, app: &mut TblApp) {
    let col = match app.context_col { Some(c) => c, None => return };
    let (group, name) = match &app.selected {
        Some(SelectedNode::Table { group, name })
        | Some(SelectedNode::Constant { group, name })
        | Some(SelectedNode::Enum { group, name }) => (group.clone(), name.clone()),
        _ => return,
    };
    let is_index_col = matches!(&app.selected, Some(SelectedNode::Table { .. }) if {
        app.find_table(&group, &name).map_or(false, |t| {
            t.schema.fields.get(col).map_or(false, |f| f.name == "id")
        })
    });
    let enum_locked = matches!(&app.selected, Some(SelectedNode::Enum { .. }));
    egui::Area::new(egui::Id::new("col_ctx"))
        .fixed_pos(app.context_pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                if enum_locked {
                    ui.add_enabled(false, egui::Button::new("枚举列固定（id|name|desc）"));
                } else {
                    if ui.button("左侧插入列").clicked() { app.insert_column(&group, &name, col); app.context_col = None; }
                    if ui.button("右侧插入列").clicked() { app.insert_column(&group, &name, col + 1); app.context_col = None; }
                    ui.separator();
                    if is_index_col {
                        ui.add_enabled(false, egui::Button::new("删除该列（主键）"));
                    } else if ui.button("删除该列").clicked() {
                        app.delete_column(&group, &name, col); app.context_col = None;
                    }
                }
            });
        });
    if ui.input(|i| i.pointer.primary_clicked() || i.key_pressed(egui::Key::Escape)) { app.context_col = None; }
}

fn render_row_context(ui: &mut egui::Ui, app: &mut TblApp) {
    let row = match app.context_row { Some(r) => r, None => return };
    let (group, name) = match &app.selected {
        Some(SelectedNode::Table { group, name })
        | Some(SelectedNode::Constant { group, name })
        | Some(SelectedNode::Enum { group, name }) => (group.clone(), name.clone()),
        _ => return,
    };
    egui::Area::new(egui::Id::new("row_ctx"))
        .fixed_pos(app.context_pos)
        .order(egui::Order::Foreground)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                if ui.button("上方插入行").clicked() { app.insert_row(&group, &name, row); app.context_row = None; }
                if ui.button("下方插入行").clicked() { app.insert_row(&group, &name, row + 1); app.context_row = None; }
                ui.separator();
                let label = match &app.edit_state.selected {
                    Selection::Rows(s, e) => format!("删除 {} 行", e - s + 1),
                    _ => "删除该行".to_string(),
                };
                if ui.button(&label).clicked() {
                    match app.edit_state.selected {
                        Selection::Rows(s, e) => { app.delete_rows(&group, &name, s, e + 1); }
                        _ => { app.delete_row(&group, &name, row); }
                    }
                    app.context_row = None;
                    app.edit_state.selected = Selection::None;
                }
            });
        });
    if ui.input(|i| i.pointer.primary_clicked() || i.key_pressed(egui::Key::Escape)) { app.context_row = None; }
}

fn render_cell_context(ui: &mut egui::Ui, app: &mut TblApp) {
    if !app.edit_state.selected.selectable() { return; }
    let (group, name) = match &app.selected {
        Some(SelectedNode::Table { group, name })
        | Some(SelectedNode::Constant { group, name })
        | Some(SelectedNode::Enum { group, name }) => (group.clone(), name.clone()),
        _ => return,
    };

    let id = egui::Id::new("cell_ctx_menu");
    let show = ui.memory(|m| m.data.get_temp::<bool>(id).unwrap_or(false));

    if ui.input(|i| i.pointer.secondary_clicked()) && app.context_col.is_none() && app.context_row.is_none() {
        app.context_pos = ui.input(|i| i.pointer.interact_pos().unwrap_or_default());
        ui.memory_mut(|m| m.data.insert_temp(id, true));
    }

    if show {
        egui::Area::new(egui::Id::new("cell_ctx_area"))
            .fixed_pos(app.context_pos)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    if ui.button("复制").clicked() {
                        if let Some(gd) = build_grid_for_selected(app) {
                            let text = super::grid::copy_selected_text(app, &gd);
                            if !text.is_empty() {
                                if let Ok(mut cb) = arboard::Clipboard::new() { let _ = cb.set_text(&text); }
                                app.log("[右键] 已复制".to_string());
                            }
                        }
                        ui.memory_mut(|m| m.data.insert_temp(id, false));
                    }
                    if ui.button("粘贴").clicked() {
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            if let Ok(text) = cb.get_text() {
                                let (sr, sc) = match app.edit_state.selected {
                                    Selection::Cell(r, c) => (r, c),
                                    Selection::CellRange { start, .. } => start,
                                    Selection::Row(r) => (r, 0),
                                    _ => (0, 0),
                                };
                                let source = match &app.selected {
                                    Some(SelectedNode::Table { .. }) => GridSource::Table,
                                    Some(SelectedNode::Enum { .. }) => GridSource::Enum,
                                    _ => GridSource::Constant,
                                };
                                app.paste_data(&group, &name, sr, sc, &text, &source);
                                app.log("[右键] 已粘贴".to_string());
                            }
                        }
                        ui.memory_mut(|m| m.data.insert_temp(id, false));
                    }
                    ui.separator();
                    if ui.button("删除内容").clicked() {
                        if let Some(gd) = build_grid_for_selected(app) {
                            app.delete_selected(&group, &name, &gd);
                        }
                        ui.memory_mut(|m| m.data.insert_temp(id, false));
                    }
                });
            });
        if ui.input(|i| i.pointer.primary_clicked() || i.key_pressed(egui::Key::Escape)) {
            ui.memory_mut(|m| m.data.insert_temp(id, false));
        }
    }
}

fn build_grid_for_selected(app: &TblApp) -> Option<GridData> {
    match &app.selected {
        Some(SelectedNode::Table { group, name }) => build_table_grid(app, group, name),
        Some(SelectedNode::Constant { group, name }) => build_constant_grid(app, group, name),
        Some(SelectedNode::Enum { group, name }) => build_enum_grid(app, group, name),
        _ => None,
    }
}

impl Selection {
    pub fn selectable(&self) -> bool {
        !matches!(self, Selection::None)
    }
}
