use eframe::egui;
use tbl_core::tblschema::*;
use crate::app::TblApp;

#[derive(Default)]
pub struct SchemaExportState {
    pub open: bool,
    pub checked: Vec<Vec<bool>>,
}

#[derive(Default)]
pub struct SchemaImportState {
    pub open: bool,
    pub file_path: String,
    pub schema: Option<TblSchema>,
    pub checked: Vec<Vec<bool>>,
    pub conflicts: Vec<Vec<bool>>,
}

pub fn render_export_dialog(ctx: &egui::Context, app: &mut TblApp) {
    if !app.schema_export.open { return; }

    let groups = &app.engine.project().groups;
    if app.schema_export.checked.len() != groups.len() {
        app.schema_export.checked = groups.iter().map(|g| {
            let count = g.tables.iter().filter(|t| !t.deleted).count()
                + g.constants.iter().filter(|c| !c.deleted).count();
            vec![true; count]
        }).collect();
    }

    let mut open = true;
    egui::Window::new("导出 Schema")
        .collapsible(false)
        .resizable(true)
        .default_width(380.0)
        .open(&mut open)
        .show(ctx, |ui| {
            render_export_content(ui, app);
        });
    if !open { app.schema_export.open = false; }
}

// PLACEHOLDER_EXPORT_CONTENT

fn render_export_content(ui: &mut egui::Ui, app: &mut TblApp) {
    let groups = &app.engine.project().groups;
    let total: usize = app.schema_export.checked.iter().map(|v| v.len()).sum();
    let selected: usize = app.schema_export.checked.iter().map(|v| v.iter().filter(|&&b| b).count()).sum();

    let all_checked = selected == total && total > 0;
    let mut all_val = all_checked;
    if ui.checkbox(&mut all_val, "全选").changed() {
        for group_checks in &mut app.schema_export.checked {
            for c in group_checks.iter_mut() { *c = all_val; }
        }
    }

    ui.separator();
    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
        for (gi, group) in groups.iter().enumerate() {
            if gi >= app.schema_export.checked.len() { break; }
            let items: Vec<(&str, bool)> = group.tables.iter().filter(|t| !t.deleted)
                .map(|t| (t.name.as_str(), true))
                .chain(group.constants.iter().filter(|c| !c.deleted).map(|c| (c.name.as_str(), false)))
                .collect();
            if items.is_empty() { continue; }

            let group_checked = app.schema_export.checked[gi].iter().all(|&b| b);
            let group_some = app.schema_export.checked[gi].iter().any(|&b| b);

            ui.horizontal(|ui| {
                let label = if group_some && !group_checked { "▣" } else if group_checked { "☑" } else { "☐" };
                if ui.selectable_label(false, format!("{} 📁 {}", label, group.name)).clicked() {
                    let new_val = !group_checked;
                    for c in app.schema_export.checked[gi].iter_mut() { *c = new_val; }
                }
            });

            for (ii, (name, is_table)) in items.iter().enumerate() {
                if ii >= app.schema_export.checked[gi].len() { break; }
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    let icon = if *is_table { "📊" } else { "📋" };
                    ui.checkbox(&mut app.schema_export.checked[gi][ii], format!("{} {}", icon, name));
                });
            }
        }
    });

    ui.separator();
    ui.label(format!("已选: {}/{} 项", selected, total));
    super::modal::dialog_buttons(ui, |ui| {
        if ui.button("取消").clicked() {
            app.schema_export.open = false;
        }
        if ui.button("导出...").clicked() && selected > 0 {
            do_export(app);
        }
    });
}

fn do_export(app: &mut TblApp) {
    let groups = &app.engine.project().groups;
    let full_schema = schema_from_project(groups, false);

    let mut selected_sections = Vec::new();
    for (gi, group) in groups.iter().enumerate() {
        let items: Vec<&str> = group.tables.iter().filter(|t| !t.deleted).map(|t| t.name.as_str())
            .chain(group.constants.iter().filter(|c| !c.deleted).map(|c| c.name.as_str()))
            .collect();
        for (ii, _) in items.iter().enumerate() {
            if gi < app.schema_export.checked.len() && ii < app.schema_export.checked[gi].len() {
                if app.schema_export.checked[gi][ii] {
                    if let Some(sec) = full_schema.sections.iter().find(|s| s.group == group.name && items[ii] == s.name) {
                        selected_sections.push(sec.clone());
                    }
                }
            }
        }
    }

    let schema = TblSchema { meta: Default::default(), sections: selected_sections };
    let content = serialize_tblschema(&schema);

    let file = rfd::FileDialog::new()
        .add_filter("TblSchema", &["tblschema"])
        .set_file_name("export.tblschema")
        .save_file();

    if let Some(path) = file {
        match std::fs::write(&path, &content) {
            Ok(_) => {
                app.log(format!("[导出Schema] 已保存到 {}", path.display()));
                app.schema_export.open = false;
            }
            Err(e) => app.log(format!("[导出Schema] 写入失败: {}", e)),
        }
    }
}

// PLACEHOLDER_IMPORT

pub fn render_import_dialog(ctx: &egui::Context, app: &mut TblApp) {
    if !app.schema_import.open { return; }

    let mut open = true;
    egui::Window::new("导入 Schema")
        .collapsible(false)
        .resizable(true)
        .default_width(450.0)
        .open(&mut open)
        .show(ctx, |ui| {
            render_import_content(ui, app);
        });
    if !open { app.schema_import.open = false; }
}

fn render_import_content(ui: &mut egui::Ui, app: &mut TblApp) {
    ui.horizontal(|ui| {
        ui.label("文件:");
        ui.add(egui::TextEdit::singleline(&mut app.schema_import.file_path)
            .desired_width(250.0));
        if ui.button("浏览...").clicked() {
            let file = rfd::FileDialog::new()
                .add_filter("TblSchema", &["tblschema"])
                .pick_file();
            if let Some(path) = file {
                app.schema_import.file_path = path.display().to_string();
                load_import_schema(app);
            }
        }
    });

    if app.schema_import.schema.is_none() {
        ui.separator();
        ui.label("请选择 .tblschema 文件");
        return;
    }

    ui.separator();

    let schema = app.schema_import.schema.as_ref().unwrap();
    let grouped = group_schema_sections(schema);
    let total: usize = app.schema_import.checked.iter().map(|v| v.len()).sum();
    let selected: usize = app.schema_import.checked.iter().map(|v| v.iter().filter(|&&b| b).count()).sum();
    let conflict_count: usize = app.schema_import.conflicts.iter().enumerate()
        .flat_map(|(gi, v)| v.iter().enumerate().map(move |(ii, &c)| (gi, ii, c)))
        .filter(|(gi, ii, c)| *c && app.schema_import.checked.get(*gi).and_then(|v| v.get(*ii)).copied().unwrap_or(false))
        .count();

    let all_checked = selected == total && total > 0;
    let mut all_val = all_checked;
    if ui.checkbox(&mut all_val, "全选").changed() {
        for group_checks in &mut app.schema_import.checked {
            for c in group_checks.iter_mut() { *c = all_val; }
        }
    }

    egui::ScrollArea::vertical().max_height(280.0).show(ui, |ui| {
        for (gi, (group_name, items)) in grouped.iter().enumerate() {
            if gi >= app.schema_import.checked.len() { break; }

            let group_checked = app.schema_import.checked[gi].iter().all(|&b| b);
            let group_some = app.schema_import.checked[gi].iter().any(|&b| b);

            ui.horizontal(|ui| {
                let label = if group_some && !group_checked { "▣" } else if group_checked { "☑" } else { "☐" };
                if ui.selectable_label(false, format!("{} 📁 {}", label, group_name)).clicked() {
                    let new_val = !group_checked;
                    for c in app.schema_import.checked[gi].iter_mut() { *c = new_val; }
                }
            });

            for (ii, (sec_name, sec_mode)) in items.iter().enumerate() {
                if ii >= app.schema_import.checked[gi].len() { break; }
                let is_conflict = app.schema_import.conflicts.get(gi).and_then(|v| v.get(ii)).copied().unwrap_or(false);
                let icon = if *sec_mode == SchemaMode::Table { "📊" } else { "📋" };
                let status = if is_conflict { "⚠️ 已存在(将覆盖)" } else { "✅ 新增" };

                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.checkbox(&mut app.schema_import.checked[gi][ii], format!("{} {}", icon, sec_name));
                    ui.label(egui::RichText::new(status).size(11.0));
                });
            }
        }
    });

    ui.separator();
    if conflict_count > 0 {
        ui.label(format!("已选: {}/{} 项 | 冲突: {} 项将覆盖", selected, total, conflict_count));
    } else {
        ui.label(format!("已选: {}/{} 项", selected, total));
    }

    super::modal::dialog_buttons(ui, |ui| {
        if ui.button("取消").clicked() {
            app.schema_import.open = false;
        }
        if ui.button("导入").clicked() && selected > 0 {
            do_import(app);
        }
    });
}

fn load_import_schema(app: &mut TblApp) {
    let content = match std::fs::read_to_string(&app.schema_import.file_path) {
        Ok(c) => c,
        Err(e) => {
            app.log(format!("[导入Schema] 读取失败: {}", e));
            app.schema_import.schema = None;
            return;
        }
    };
    match parse_tblschema(&content) {
        Ok(schema) => {
            let grouped = group_schema_sections(&schema);
            let groups = &app.engine.project().groups;

            let mut checked = Vec::new();
            let mut conflicts = Vec::new();
            for (group_name, items) in &grouped {
                let mut gc = Vec::new();
                let mut gf = Vec::new();
                for (name, mode) in items {
                    gc.push(true);
                    let exists = if let Some(g) = groups.iter().find(|g| &g.name == group_name) {
                        match mode {
                            SchemaMode::Table => g.tables.iter().any(|t| &t.name == name && !t.deleted),
                            SchemaMode::Constant => g.constants.iter().any(|c| &c.name == name && !c.deleted),
                            SchemaMode::Enum => g.enums.iter().any(|e| &e.name == name && !e.deleted),
                        }
                    } else { false };
                    gf.push(exists);
                }
                checked.push(gc);
                conflicts.push(gf);
            }

            app.schema_import.checked = checked;
            app.schema_import.conflicts = conflicts;
            app.schema_import.schema = Some(schema);
        }
        Err(e) => {
            app.log(format!("[导入Schema] 解析失败: {}", e));
            app.schema_import.schema = None;
        }
    }
}

fn do_import(app: &mut TblApp) {
    let schema = match &app.schema_import.schema {
        Some(s) => s.clone(),
        None => return,
    };

    let grouped = group_schema_sections(&schema);
    let mut selected_sections = Vec::new();

    for (gi, (_, items)) in grouped.iter().enumerate() {
        for (ii, _) in items.iter().enumerate() {
            if app.schema_import.checked.get(gi).and_then(|v| v.get(ii)).copied().unwrap_or(false) {
                if let Some(sec) = schema.sections.iter().find(|s| {
                    s.group == grouped[gi].0 && s.name == items[ii].0
                }) {
                    selected_sections.push(sec.clone());
                }
            }
        }
    }

    let config_dir = app.engine.project().data_dir();
    let (added, overwritten) = apply_schema_to_project(
        &mut app.engine.project_mut().groups,
        &selected_sections,
        &config_dir,
        false,
    );

    app.log(format!("[导入Schema] 完成: {} 新增, {} 覆盖", added, overwritten));
    app.schema_import.open = false;
}

fn group_schema_sections(schema: &TblSchema) -> Vec<(String, Vec<(String, SchemaMode)>)> {
    let mut result: Vec<(String, Vec<(String, SchemaMode)>)> = Vec::new();
    for sec in &schema.sections {
        if let Some(entry) = result.iter_mut().find(|(g, _)| *g == sec.group) {
            entry.1.push((sec.name.clone(), sec.mode.clone()));
        } else {
            result.push((sec.group.clone(), vec![(sec.name.clone(), sec.mode.clone())]));
        }
    }
    result
}

// --- 导出数据对话框 ---

#[derive(Clone)]
pub struct DataExportState {
    pub open: bool,
    pub json: bool,
    pub xml: bool,
    pub java: bool,
    pub go: bool,
    pub lua: bool,
}

impl Default for DataExportState {
    fn default() -> Self {
        Self { open: false, json: true, xml: true, java: true, go: false, lua: true }
    }
}

impl DataExportState {
    fn set_all(&mut self) { self.json = true; self.xml = true; self.java = true; self.go = true; self.lua = true; }
    fn set_data(&mut self) { self.json = true; self.xml = true; self.java = false; self.go = false; self.lua = false; }
    fn set_server(&mut self) { self.json = true; self.xml = true; self.java = true; self.go = true; self.lua = false; }
    fn set_client(&mut self) { self.json = false; self.xml = false; self.java = false; self.go = false; self.lua = true; }

    fn any_selected(&self) -> bool {
        self.json || self.xml || self.java || self.go || self.lua
    }
}

pub fn render_data_export_dialog(ctx: &egui::Context, app: &mut TblApp) {
    if !app.data_export.open { return; }

    let mut open = true;
    egui::Window::new("导出")
        .collapsible(false)
        .resizable(false)
        .default_width(280.0)
        .open(&mut open)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("全部").clicked() { app.data_export.set_all(); }
                if ui.button("数据").clicked() { app.data_export.set_data(); }
                if ui.button("后端").clicked() { app.data_export.set_server(); }
                if ui.button("前端").clicked() { app.data_export.set_client(); }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.checkbox(&mut app.data_export.json, "JSON");
                ui.checkbox(&mut app.data_export.xml, "XML");
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut app.data_export.java, "Java");
                ui.checkbox(&mut app.data_export.go, "Go");
            });
            ui.horizontal(|ui| {
                ui.checkbox(&mut app.data_export.lua, "Lua");
            });
            ui.separator();
            super::modal::dialog_buttons(ui, |ui| {
                if ui.button("取消").clicked() {
                    app.data_export.open = false;
                }
                let enabled = app.data_export.any_selected();
                if ui.add_enabled(enabled, egui::Button::new("导出")).clicked() {
                    do_data_export(app);
                }
            });
        });
    if !open { app.data_export.open = false; }
}

fn do_data_export(app: &mut TblApp) {
    let state = app.data_export.clone();
    app.data_export.open = false;

    if state.json {
        match app.engine.export_json() {
            Ok(r) => log_export_result(app, "JSON", &r),
            Err(e) => app.log(format!("[JSON] 错误: {}", e)),
        }
    }
    if state.xml {
        match app.engine.export_xml() {
            Ok(r) => log_export_result(app, "XML", &r),
            Err(e) => app.log(format!("[XML] 错误: {}", e)),
        }
    }
    if state.java {
        match app.engine.export_java() {
            Ok(r) => log_export_result(app, "Java", &r),
            Err(e) => app.log(format!("[Java] 错误: {}", e)),
        }
    }
    if state.go {
        match app.engine.export_go() {
            Ok(r) => log_export_result(app, "Go", &r),
            Err(e) => app.log(format!("[Go] 错误: {}", e)),
        }
    }
    if state.lua {
        match app.engine.export_lua() {
            Ok(r) => log_export_result(app, "Lua", &r),
            Err(e) => app.log(format!("[Lua] 错误: {}", e)),
        }
    }
}

fn log_export_result(app: &mut TblApp, label: &str, result: &tbl_core::export::ExportResult) {
    use tbl_core::export::FileStatus;
    app.log(format!("[{}] {} 新增, {} 修改, {} 删除, {} 不变",
        label, result.added(), result.modified(), result.deleted(), result.unchanged()));
    for f in &result.files {
        match f.status {
            FileStatus::Added => app.log(format!("  [新增] {}", f.path)),
            FileStatus::Modified => app.log(format!("  [修改] {}", f.path)),
            FileStatus::Deleted => app.log(format!("  [删除] {}", f.path)),
            FileStatus::Unchanged => {}
        }
    }
}